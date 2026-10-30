# CCS-184: [sessions] Support session resume on Windows and Linux

**Status:** Partial · **Kind:** feature · **Labels:** `feature`, `phase:P5`, `p3-low`, `area:sessions`

> **Ticket key:** `p5-session-resume-cross-platform` · **Phase:** P5 · **Priority:** p3-low · **Effort:** M · **Depends on:** `p5-misc-split-terminal`, `p1-ipc-session-terminal`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Session resume is macOS-only, although Linux and Windows launchers exist.

## Proposed change
Use platform/terminal for resume on all 3 OSes through the backend-built command from p1-ipc-session-terminal.

## Acceptance criteria / verification
- [x] Per-OS unit tests of the generated launch script
- [ ] Manual resume on Windows and Linux
- [x] `cargo test --manifest-path src-tauri/Cargo.toml terminal`

## Dependencies
p5-misc-split-terminal, p1-ipc-session-terminal

## Implementation notes

**Summary.** `launch_session_terminal` used to fail outright on non-macOS hosts
(`"Terminal resume is only supported on macOS"`), and the session page hid the resume button
unless `isMac()`. Resume now works on all three OSes. It still uses the backend-built command
from CCS-074, and the webview still sends only `{providerId, sessionId}`.
- `session_manager::ResumeTarget` gained `argv`, the same fixed template as an unquoted
  argument list (`build_resume_argv`, which shares `resume_program` with `build_resume_command`).
- `platform::terminal::launch_resume_terminal(target, &ResumeTarget, custom)` dispatches by
  `cfg`. On **macOS** the existing per-target AppleScript / `open` launchers are unchanged.
  On **Linux** and **Windows** the session `cwd` is validated with `resolve_launch_cwd`
  (missing directory = error, and the UI falls back to copying the command), then the backend
  writes a private temp launcher and hands it to the preferred-terminal logic that the provider
  terminals already use.
  - Linux: `build_posix_resume_script` writes a POSIX sh script. It runs the POSIX-quoted
    resume command through the user's shell in interactive mode (`'<shell>' -ic 'cd <dir> &&
    <cmd>'`, the same helper as the provider launcher, now extracted as
    `posix::build_user_shell_command_line`) so the user's PATH loads. It then removes itself and
    `exec`s the interactive shell in the session directory, so the window stays open.
  - Windows: `build_windows_resume_batch` writes a `.bat`: `cd /d "<dir>"` (or `pushd` for UNC),
    then `call <cli> <args>` (`call` makes npm `.cmd` shims return), then a self-delete. It runs
    through `cmd /K`, PowerShell `-NoExit` or `wt`. cmd re-parses `%`, `!`, `^` and `"` in
    batch files and shims, so the builder accepts only `[A-Za-z0-9._-]` arguments; any other
    session id is rejected and the UI falls back to copying the command.
- Frontend: `SessionManagerPage` shows the resume button on every platform and always calls
  `sessionsApi.launchTerminal`. On error it still copies the command and shows a toast.

**Files.** `src-tauri/src/session_manager/mod.rs`, `src-tauri/src/commands/session_manager.rs`,
`src-tauri/src/platform/terminal/{resume,posix,linux,windows}.rs`,
`src/components/sessions/SessionManagerPage.tsx`, `tests/components/SessionManagerPage.test.tsx`.

**Commits.** `c9a0414` (code + tests); this ticket-notes commit follows it.

**Tests added.**
- `platform::terminal::resume::tests::posix_resume_script_runs_command_in_session_dir_then_execs_shell`
  and `posix_resume_script_keeps_quoted_session_id_as_single_argument` (exact Linux script).
- `posix_resume_script_executes_resume_in_session_dir_and_self_deletes` (unix only). It really
  runs the generated script with `sh` and a fake `claude` on PATH, then checks three things:
  the CLI runs in the session dir, it gets `abc;touch pwned` as a single argument and nothing
  is injected, and the script deletes itself.
- `windows_resume_batch_changes_dir_and_calls_cli`,
  `windows_resume_batch_uses_pushd_for_unc_and_skips_missing_cwd` and
  `windows_resume_batch_rejects_args_cmd_would_reinterpret` (exact `.bat` content). These are
  pure builders, so they also run on Linux CI.
- `session_manager::tests::resume_argv_matches_fixed_templates_without_quoting`; the CCS-074
  `ResumeTarget` test now also asserts `argv`.
- `SessionManagerPage` "resumes the selected session in a terminal on non-macOS platforms"
  (jsdom UA is not macOS). This test fails on the old code: the button did not render, and
  clicking it only copied the command.

**Verification (Linux).**
- `cargo fmt`: clean. `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings` and
  `--lib --tests -- -D warnings`: clean.
- `cargo test --manifest-path src-tauri/Cargo.toml terminal`: all targets ok (lib: 32 passed).
  `--lib resume`: 18 passed. `--lib session_manager`: 77 passed.
- `cargo test --manifest-path src-tauri/Cargo.toml --lib`: 2799 passed, 0 failed, 3 ignored.
- Windows cross-check: `cargo clippy --target x86_64-pc-windows-gnu --lib --tests` (mingw-w64)
  shows no warnings in any file this ticket touches. The only warnings are pre-existing ones in
  `secret_store.rs`, `settings.rs`, `commands/settings.rs` and `tests/profile_roundtrip.rs`.
- `pnpm typecheck`: clean. `pnpm test:unit`: 164 files / 1570 tests passed.
  `pnpm format:check`: clean.

**Remaining (why Partial).**
- Manual resume on a real Windows desktop and a real Linux desktop (GNOME/KDE terminal) has not
  been done. This container has no display, and with only about 2.5 GB of free disk the Windows
  test binary could not be built and run under Wine (CCS-179 ran the same kind of check).
  To do it: open Sessions, select a Claude/Codex session, click Resume, and confirm a terminal
  opens in the project directory, runs `<cli> --resume <id>`, and stays open afterwards.

