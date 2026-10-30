//! 工具安装 / 更新脚本的流式执行：逐行转发 stdout/stderr、可取消、带超时。
//!
//! `run_tool_lifecycle_action` 以前阻塞到子进程结束，期间前端看不到任何输出，
//! 卡死的安装器（网络挂起、等待交互输入）也没有上限。这里改为 `tokio::process`
//! 启动，按行回调输出；取消与超时都会杀掉整个进程树（bash 派生的 npm / curl 等）。

use crate::platform::process::decode_command_output;
#[cfg(target_os = "windows")]
use crate::platform::process::CREATE_NO_WINDOW;
use std::collections::{HashMap, VecDeque};
use std::ffi::OsString;
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{LazyLock, Mutex};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncRead, BufReader};
use tokio::sync::{mpsc, watch};

/// 前端订阅的逐行输出事件名。
pub const TOOL_LIFECYCLE_OUTPUT_EVENT: &str = "tool-lifecycle-output";

/// 未指定超时时的默认上限：npm 全局安装在慢网络下可能要几分钟，留足余量。
pub const DEFAULT_TOOL_LIFECYCLE_TIMEOUT: Duration = Duration::from_secs(20 * 60);

/// 失败时错误详情保留的末尾行数（与旧的静默执行一致）。
const ERROR_TAIL_LINES: usize = 8;

/// 子进程退出后等待输出管道排空的宽限期：被脚本放到后台的孙进程可能一直持有管道。
const DRAIN_GRACE: Duration = Duration::from_secs(2);

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum LifecycleStream {
    Stdout,
    Stderr,
}

/// `tool-lifecycle-output` 事件载荷。
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolLifecycleOutputLine {
    pub run_id: String,
    pub stream: LifecycleStream,
    pub line: String,
}

/// 进行中的运行：run_id → (登记序号, 取消信号发送端)。序号让守卫只注销自己那次登记。
type RunRegistry = HashMap<String, (u64, watch::Sender<bool>)>;

static ACTIVE_RUNS: LazyLock<Mutex<RunRegistry>> = LazyLock::new(|| Mutex::new(HashMap::new()));
static NEXT_RUN_SEQ: AtomicU64 = AtomicU64::new(0);

/// 登记一次运行；返回的守卫在 drop 时注销，接收端交给 [`run_streaming`]。
pub(super) fn register_run(run_id: &str) -> (RunGuard, watch::Receiver<bool>) {
    let (tx, rx) = watch::channel(false);
    let seq = NEXT_RUN_SEQ.fetch_add(1, Ordering::Relaxed);
    let mut runs = ACTIVE_RUNS.lock().unwrap_or_else(|e| e.into_inner());
    // 同一 run_id 重复登记时，先取消旧的那次，避免它变成无法取消的孤儿。
    if let Some((_, previous)) = runs.insert(run_id.to_string(), (seq, tx)) {
        let _ = previous.send(true);
    }
    (
        RunGuard {
            run_id: run_id.to_string(),
            seq,
        },
        rx,
    )
}

pub(super) struct RunGuard {
    run_id: String,
    seq: u64,
}

impl Drop for RunGuard {
    fn drop(&mut self) {
        let mut runs = ACTIVE_RUNS.lock().unwrap_or_else(|e| e.into_inner());
        if runs
            .get(&self.run_id)
            .is_some_and(|(seq, _)| *seq == self.seq)
        {
            runs.remove(&self.run_id);
        }
    }
}

/// 取消指定运行；`run_id` 为 `None` 时取消全部。返回是否命中了进行中的运行。
pub fn cancel_run(run_id: Option<&str>) -> bool {
    let runs = ACTIVE_RUNS.lock().unwrap_or_else(|e| e.into_inner());
    match run_id {
        Some(id) => runs
            .get(id)
            .map(|(_, tx)| tx.send(true).is_ok())
            .unwrap_or(false),
        None => {
            let mut hit = false;
            for (_, tx) in runs.values() {
                hit |= tx.send(true).is_ok();
            }
            hit
        }
    }
}

/// 启动 `program args...`，逐行回调输出，直到退出 / 取消 / 超时。
///
/// - 正常退出码 0 → `Ok(())`；非 0 → stderr（空则 stdout）末尾若干行。
/// - 取消或超时 → 杀掉整个进程树并返回对应错误。
pub(super) async fn run_streaming<F>(
    program: &str,
    args: &[OsString],
    timeout: Duration,
    mut cancel: watch::Receiver<bool>,
    mut on_line: F,
) -> Result<(), String>
where
    F: FnMut(LifecycleStream, String) + Send,
{
    if *cancel.borrow() {
        return Err(cancelled_message());
    }

    let mut command = tokio::process::Command::new(program);
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    // 独立进程组，取消/超时时能一次杀掉 bash 派生的所有子孙进程。
    #[cfg(unix)]
    command.process_group(0);
    #[cfg(target_os = "windows")]
    command.creation_flags(CREATE_NO_WINDOW);

    let mut child = command
        .spawn()
        .map_err(|e| format!("启动安装进程失败: {e}"))?;

    let (tx, mut rx) = mpsc::unbounded_channel::<(LifecycleStream, String)>();
    let mut readers = Vec::new();
    if let Some(stdout) = child.stdout.take() {
        readers.push(tokio::spawn(forward_lines(
            stdout,
            LifecycleStream::Stdout,
            tx.clone(),
        )));
    }
    if let Some(stderr) = child.stderr.take() {
        readers.push(tokio::spawn(forward_lines(
            stderr,
            LifecycleStream::Stderr,
            tx.clone(),
        )));
    }
    drop(tx);

    let mut stdout_tail = VecDeque::with_capacity(ERROR_TAIL_LINES);
    let mut stderr_tail = VecDeque::with_capacity(ERROR_TAIL_LINES);
    let deadline = tokio::time::sleep(timeout);
    tokio::pin!(deadline);
    let drain = tokio::time::sleep(Duration::MAX);
    tokio::pin!(drain);

    let mut status = None;
    let mut readers_done = false;
    let mut cancel_closed = false;

    let outcome = loop {
        if status.is_some() && readers_done {
            break Ok(());
        }
        tokio::select! {
            biased;
            msg = rx.recv(), if !readers_done => match msg {
                Some((stream, line)) => {
                    let tail = match stream {
                        LifecycleStream::Stdout => &mut stdout_tail,
                        LifecycleStream::Stderr => &mut stderr_tail,
                    };
                    if tail.len() == ERROR_TAIL_LINES {
                        tail.pop_front();
                    }
                    tail.push_back(line.clone());
                    on_line(stream, line);
                }
                None => readers_done = true,
            },
            res = child.wait(), if status.is_none() => {
                status = Some(res.map_err(|e| format!("等待安装进程失败: {e}"))?);
                drain
                    .as_mut()
                    .reset(tokio::time::Instant::now() + DRAIN_GRACE);
            }
            _ = &mut drain, if status.is_some() => break Ok(()),
            changed = cancel.changed(), if !cancel_closed => {
                if changed.is_err() {
                    cancel_closed = true;
                } else if *cancel.borrow() {
                    break Err(cancelled_message());
                }
            }
            _ = &mut deadline => {
                break Err(format!("安装/更新超时（{} 秒），已终止进程", timeout.as_secs()));
            }
        }
    };

    if outcome.is_err() {
        kill_process_tree(&mut child).await;
    }
    for reader in readers {
        reader.abort();
    }
    outcome?;

    let status = status.expect("loop only exits Ok after the child exited");
    if status.success() {
        return Ok(());
    }
    let tail = if stderr_tail.iter().all(|l| l.trim().is_empty()) {
        stdout_tail
    } else {
        stderr_tail
    };
    let detail = tail.into_iter().collect::<Vec<_>>().join("\n");
    let detail = detail.trim();
    Err(if detail.is_empty() {
        format!("命令执行失败 (exit code: {:?})", status.code())
    } else {
        detail.to_string()
    })
}

fn cancelled_message() -> String {
    "安装/更新已取消".to_string()
}

/// 按行读取管道并转发；按字节切行再解码，避免非 UTF-8 输出（Windows 代码页）中断读取。
async fn forward_lines<R>(
    reader: R,
    stream: LifecycleStream,
    tx: mpsc::UnboundedSender<(LifecycleStream, String)>,
) where
    R: AsyncRead + Unpin,
{
    let mut reader = BufReader::new(reader);
    let mut buf = Vec::new();
    loop {
        buf.clear();
        match reader.read_until(b'\n', &mut buf).await {
            Ok(0) | Err(_) => break,
            Ok(_) => {
                let line = decode_command_output(&buf);
                let line = line.trim_end_matches(['\r', '\n']).to_string();
                if tx.send((stream, line)).is_err() {
                    break;
                }
            }
        }
    }
}

async fn kill_process_tree(child: &mut tokio::process::Child) {
    #[cfg(unix)]
    {
        if let Some(pid) = child.id().and_then(|pid| i32::try_from(pid).ok()) {
            // SAFETY: 向 spawn 时创建的进程组（pgid == pid）发送 SIGKILL，不涉及内存操作。
            unsafe {
                libc::kill(-pid, libc::SIGKILL);
            }
        }
    }
    #[cfg(target_os = "windows")]
    {
        if let Some(pid) = child.id() {
            use std::os::windows::process::CommandExt;
            let _ = std::process::Command::new("taskkill")
                .args(["/PID", &pid.to_string(), "/T", "/F"])
                .creation_flags(CREATE_NO_WINDOW)
                .output();
        }
    }
    let _ = child.kill().await;
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::time::Instant;

    /// 写一个假安装器脚本，返回 `bash <script>` 的参数。
    fn fake_installer(dir: &tempfile::TempDir, body: &str) -> Vec<OsString> {
        let path = dir.path().join("fake-installer.sh");
        std::fs::write(&path, body).expect("write fake installer");
        vec![path.into_os_string()]
    }

    /// 进程已不存在（或只剩待回收的僵尸）即视为被杀掉。
    async fn wait_until_dead(pid: u32) -> bool {
        for _ in 0..50 {
            match std::fs::read_to_string(format!("/proc/{pid}/stat")) {
                Err(_) => return true,
                Ok(stat) => {
                    let state = stat.rsplit(')').next().unwrap_or("").trim_start();
                    if state.starts_with('Z') || state.starts_with('X') {
                        return true;
                    }
                }
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        false
    }

    fn never_cancelled() -> watch::Receiver<bool> {
        let (tx, rx) = watch::channel(false);
        // 发送端泄漏到测试结束，保持通道打开
        std::mem::forget(tx);
        rx
    }

    #[tokio::test]
    async fn streams_lines_from_both_pipes_in_order() {
        let dir = tempfile::tempdir().unwrap();
        let args = fake_installer(
            &dir,
            "echo 'resolving packages'\necho 'npm warn deprecated' >&2\nprintf 'added 1 package\\r\\n'\n",
        );
        let mut lines = Vec::new();
        let result = run_streaming(
            "bash",
            &args,
            Duration::from_secs(30),
            never_cancelled(),
            |stream, line| lines.push((stream, line)),
        )
        .await;

        assert_eq!(result, Ok(()));
        let stdout: Vec<_> = lines
            .iter()
            .filter(|(s, _)| *s == LifecycleStream::Stdout)
            .map(|(_, l)| l.as_str())
            .collect();
        assert_eq!(stdout, ["resolving packages", "added 1 package"]);
        assert!(lines.contains(&(LifecycleStream::Stderr, "npm warn deprecated".to_string())));
    }

    #[tokio::test]
    async fn failure_returns_stderr_tail() {
        let dir = tempfile::tempdir().unwrap();
        let args = fake_installer(
            &dir,
            "echo progress\nfor i in $(seq 1 12); do echo \"err $i\" >&2; done\nexit 3\n",
        );
        let err = run_streaming(
            "bash",
            &args,
            Duration::from_secs(30),
            never_cancelled(),
            |_, _| {},
        )
        .await
        .unwrap_err();
        let expected: Vec<String> = (5..=12).map(|i| format!("err {i}")).collect();
        assert_eq!(err, expected.join("\n"));
    }

    #[tokio::test]
    async fn failure_without_stderr_falls_back_to_stdout_or_exit_code() {
        let dir = tempfile::tempdir().unwrap();
        let args = fake_installer(&dir, "echo 'only stdout'\nexit 2\n");
        let err = run_streaming(
            "bash",
            &args,
            Duration::from_secs(30),
            never_cancelled(),
            |_, _| {},
        )
        .await
        .unwrap_err();
        assert_eq!(err, "only stdout");

        let args = fake_installer(&dir, "exit 4\n");
        let err = run_streaming(
            "bash",
            &args,
            Duration::from_secs(30),
            never_cancelled(),
            |_, _| {},
        )
        .await
        .unwrap_err();
        assert_eq!(err, "命令执行失败 (exit code: Some(4))");
    }

    #[tokio::test]
    async fn cancel_kills_the_installer_and_its_children() {
        let dir = tempfile::tempdir().unwrap();
        // 安装器派生一个长时间运行的子进程并打印其 pid，然后等待它。
        let args = fake_installer(&dir, "sleep 30 &\necho \"child $!\"\nwait\n");
        let run_id = "test-cancel-kills-installer";
        let (_guard, cancel) = register_run(run_id);

        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let mut started_tx = Some(started_tx);
        let canceller = tokio::spawn(async move {
            let pid: u32 = started_rx.await.unwrap();
            assert!(cancel_run(Some(run_id)));
            pid
        });

        let begin = Instant::now();
        let result = run_streaming("bash", &args, Duration::from_secs(60), cancel, |_, line| {
            if let Some(pid) = line.strip_prefix("child ") {
                if let Some(tx) = started_tx.take() {
                    let _ = tx.send(pid.parse().unwrap());
                }
            }
        })
        .await;
        let child_pid = canceller.await.unwrap();

        assert_eq!(result, Err("安装/更新已取消".to_string()));
        assert!(begin.elapsed() < Duration::from_secs(10));
        assert!(
            wait_until_dead(child_pid).await,
            "grandchild {child_pid} survived cancel"
        );
    }

    #[tokio::test]
    async fn cancel_before_spawn_never_starts_the_process() {
        let dir = tempfile::tempdir().unwrap();
        let marker = dir.path().join("ran");
        let args = fake_installer(&dir, &format!("touch '{}'\n", marker.display()));
        let run_id = "test-cancel-before-spawn";
        let (_guard, cancel) = register_run(run_id);
        assert!(cancel_run(Some(run_id)));

        let result = run_streaming("bash", &args, Duration::from_secs(30), cancel, |_, _| {}).await;
        assert_eq!(result, Err("安装/更新已取消".to_string()));
        assert!(!marker.exists());
    }

    #[tokio::test]
    async fn timeout_kills_the_installer_and_returns_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let args = fake_installer(&dir, "sleep 30 &\necho \"child $!\"\nwait\n");
        let begin = Instant::now();
        let mut lines = Vec::new();
        let result = run_streaming(
            "bash",
            &args,
            Duration::from_millis(500),
            never_cancelled(),
            |_, line| lines.push(line),
        )
        .await;

        let err = result.unwrap_err();
        assert!(err.contains("超时"), "unexpected error: {err}");
        assert!(begin.elapsed() < Duration::from_secs(10));
        assert_eq!(lines.len(), 1);
        let child_pid: u32 = lines[0].strip_prefix("child ").unwrap().parse().unwrap();
        assert!(
            wait_until_dead(child_pid).await,
            "grandchild {child_pid} survived timeout"
        );
    }

    #[test]
    fn guard_drop_unregisters_and_cancel_misses_unknown_runs() {
        let run_id = "test-guard-unregisters";
        {
            let (_guard, _rx) = register_run(run_id);
            assert!(ACTIVE_RUNS.lock().unwrap().contains_key(run_id));
        }
        assert!(!ACTIVE_RUNS.lock().unwrap().contains_key(run_id));
        assert!(!cancel_run(Some(run_id)));
    }

    #[test]
    fn re_registering_a_run_id_cancels_the_previous_run() {
        let run_id = "test-re-register";
        let (first_guard, first_rx) = register_run(run_id);
        let (_second_guard, second_rx) = register_run(run_id);
        assert!(*first_rx.borrow());
        assert!(!*second_rx.borrow());
        // 旧守卫注销时不能顺手把新登记的运行也移除
        drop(first_guard);
        assert!(cancel_run(Some(run_id)));
        assert!(*second_rx.borrow());
    }
}
