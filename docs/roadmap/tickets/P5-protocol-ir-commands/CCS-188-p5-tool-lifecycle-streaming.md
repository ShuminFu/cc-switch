# CCS-188: [cli-tools] Stream installer output and support cancel and timeout for tool lifecycle actions

**Status:** Done · **Kind:** feature · **Labels:** `feature`, `phase:P5`, `p3-low`, `area:cli-tools`

> **Ticket key:** `p5-tool-lifecycle-streaming` · **Phase:** P5 · **Priority:** p3-low · **Effort:** M · **Depends on:** `p5-misc-split-terminal`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Backend idea 9. run_tool_lifecycle_action blocks until the command ends, with no output and no limit.

## Proposed change
Spawn with tokio::process, emit line events, and add a cancel command and a configurable timeout.

## Acceptance criteria / verification
- [x] Test with a fake installer script: lines arrive as events, cancel kills it, and a timeout returns an error
- [x] `cargo test --manifest-path src-tauri/Cargo.toml cli_tools` and `pnpm test:unit`

## Dependencies
p5-misc-split-terminal

## Implementation notes

**Summary.** `run_tool_lifecycle_action` no longer blocks silently on `std::process::Command::output()`.
The new `services/cli_tools/lifecycle_run.rs` spawns the generated script with `tokio::process`
(`bash -c` on Unix, `cmd /C <tmp>.bat` with `CREATE_NO_WINDOW` on Windows), reads stdout/stderr
line by line (byte-split then `decode_command_output`, so Windows code pages still work) and
hands each line to a callback. The command layer emits it as the `tool-lifecycle-output` event
(`{ runId, stream: "stdout" | "stderr", line }`).

- **Cancel:** each run is registered by `runId` (optional command argument; generated when absent)
  in a process-wide registry. The new `cancel_tool_lifecycle_action(runId?)` command signals one
  run (or all when `runId` is omitted) and returns whether it hit a running action. Cancelling
  during the anchoring probe also works, because the signal is checked before the process starts.
- **Timeout:** the new `timeoutSecs` argument (default `DEFAULT_TOOL_LIFECYCLE_TIMEOUT` = 20 min).
  When it expires the run returns `安装/更新超时（N 秒），已终止进程`.
- On cancel or timeout the whole process tree is killed: the child is spawned in its own process
  group and gets `kill(-pgid, SIGKILL)` on Unix, `taskkill /T /F` on Windows, plus `kill_on_drop`.
  On a non-zero exit the error detail is still the last 8 stderr lines (stdout when stderr is
  empty), or the exit code, same as before.
- `lifecycle_cmd.rs`: `run_tool_lifecycle_silently` / `finish_lifecycle_output` were replaced by
  `lifecycle_invocation` (program + args + Windows temp .bat that is removed on drop).
- Frontend: `settingsApi.runToolLifecycleAction(..., { runId, timeoutSecs })`,
  `settingsApi.cancelToolLifecycleAction(runId)`, `settingsApi.onToolLifecycleOutput(handler)`.
  The About page tool cards show the latest output line and a Cancel button while an action runs.
  A cancelled run shows an info toast instead of an error and stops the rest of a batch. New i18n
  keys `settings.toolCancel` / `settings.toolActionCancelled` are in all four locales.

**Files.** `src-tauri/src/services/cli_tools/lifecycle_run.rs` (new), `src-tauri/src/services/cli_tools/{mod.rs,lifecycle_cmd.rs}`,
`src-tauri/src/commands/cli_tools.rs`, `src-tauri/src/lib.rs` (register `cancel_tool_lifecycle_action`),
`src-tauri/Cargo.toml` (tokio `io-util` declared explicitly), `src/shared/bindings.ts` (regenerated `COMMANDS`),
`src/lib/api/settings.ts`, `src/components/settings/AboutSection.tsx`, `src/i18n/locales/*.json`,
`tests/msw/unmockedCommands.ts`, `tests/lib/toolLifecycleApi.test.ts` (new).

**Commits.** `df5a1fa` feat(cli-tools): stream installer output with cancel and timeout (CCS-188); ticket notes in the follow-up docs commit.

**Tests (fake installer scripts, `lifecycle_run::tests`, Unix).**
- `streams_lines_from_both_pipes_in_order`: stdout and stderr lines arrive through the callback, with CRLF trimmed.
- `failure_returns_stderr_tail` / `failure_without_stderr_falls_back_to_stdout_or_exit_code`.
- `cancel_kills_the_installer_and_its_children`: cancel by `runId` through the registry returns the cancelled error and kills a backgrounded grandchild `sleep`.
- `cancel_before_spawn_never_starts_the_process`.
- `timeout_kills_the_installer_and_returns_an_error`: a 500 ms timeout returns the timeout error and kills the grandchild.
- Registry: guard drop unregisters, and re-registering a `runId` cancels the old run without the old guard removing the new one.
- To check that the tests catch a regression, the process-group `SIGKILL` was disabled temporarily. Both the cancel and timeout tests then failed because the grandchild survived.
- Vitest `tests/lib/toolLifecycleApi.test.ts`: `runId`/`timeoutSecs` forwarding, cancel invoke, event subscription and unlisten.

**Verification.**
- `cargo fmt --check`: clean.
- `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings`: clean. Also clean with `--lib --tests`.
- `cargo check --lib --target x86_64-pc-windows-gnu`: OK. This compiles the Windows `taskkill`/`creation_flags`/.bat path.
- `cargo test --manifest-path src-tauri/Cargo.toml --lib cli_tools`: 71 passed, 0 failed. The `lifecycle_run` tests were run 3 more times with no flakes.
- `cargo test --lib`: 2807 passed, 0 failed, 3 ignored.
- `cargo test --lib bindings`: 2 passed, after `UPDATE_BINDINGS=1` regeneration.
- `pnpm typecheck`: OK.
- `pnpm test:unit`: 165 files, 1573 tests passed.
- `pnpm format:check`: clean.
- Not verified locally: running the Windows path on a real Windows host, where `taskkill` kills the tree. macOS uses the same Unix code path as Linux.
