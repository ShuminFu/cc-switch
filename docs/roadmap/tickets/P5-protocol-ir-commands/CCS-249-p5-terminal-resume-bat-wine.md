# CCS-249: [terminal] Run the generated Windows resume .bat under Wine cmd

**Status:** Done · **Kind:** chore · **Labels:** `chore`, `phase:P5`, `p3-low`, `area:terminal`

> **Ticket key:** `p5-terminal-resume-bat-wine` · **Phase:** P5 · **Priority:** p3-low · **Effort:** S
> Plan: [next-phase-plan.md](../../next-phase-plan.md)
> Found during the Partial-ticket triage at d962272 (CCS-184). Source tickets: [CCS-184](../P5-protocol-ir-commands/CCS-184-p5-session-resume-cross-platform.md).

## Problem
The Windows resume batch (`build_windows_resume_batch`, `src-tauri/src/platform/terminal/resume.rs:101`) had a caret-escaping fix in review but has never been executed. Wine is installed and `wine cmd /c t.bat` runs batch files here; no Windows cross-compile is needed because the builder is OS-neutral.

## Proposed change
- Add an `#[ignore]` test or `scripts/verify-windows-resume-bat.sh` that builds the batch on Linux with a cwd like `Z:\tmp\proj (1)\100%` and a session-id-like arg, writes it next to a fake `claude.cmd` shim that echoes `%CD%` and `%*` to a marker, and runs `wine cmd /c <bat>` (and `wine cmd /K` with stdin closed).
- Assert: `cd /d` into the quoted path succeeded, the shim got exactly `--resume <id>`, execution continued after `call`, `del "%~f0"` removed the .bat.
- Record in CCS-184 that this covers the .bat content only, not how cmd/wt/PowerShell launch it.

## Acceptance criteria / verification
- [x] The Wine run passes all four assertions.
- [x] `cargo test --manifest-path src-tauri/Cargo.toml --lib platform::terminal` still passes.

## Dependencies
None.

## Implementation notes

**Change.** The ignored test `platform::terminal::resume::tests::windows_resume_batch_runs_under_wine_cmd` in `src-tauri/src/platform/terminal/resume.rs` builds the resume batch on Linux. It uses argv `claude --resume 0199a1b2-c3d4.e5_f6` and the cwd `Z:\tmp\.tmpXXXX\proj (1)&x^y\100%`, which has a space, parentheses, `&`, `^` and `%`. The batch is written next to a fake `claude.cmd` shim, which is on `WINEPATH`. The shim uses delayed expansion to write `CD=[!CD!]` and `ARGS=[%*]` to a marker file. The test runs `wine cmd /c <bat>` and then `wine cmd /K <bat>` with stdin closed, each in a fresh temp dir, and asserts the following:
1. `cd /d` entered the quoted dir (the marker CD matches the session dir exactly);
2. the shim got exactly `--resume <id>`;
3. the batch kept going after `call`;
4. `del "%~f0"` removed the batch, which also proves item 3.

For `/K`, it also asserts that cmd stays on a prompt in the session dir.

**Wine 9.0 caveat.** Wine 9.0's cmd sets no return code for builtins. Here `cd /d Z:\tmp || echo FAIL`, `pushd ... ||`, `echo x ||` and `ver ||` all take the `||` branch, and `&&` runs even after a failed `cd`. So the unmodified batch exits at `cd /d "..." || exit /b 1` even when the cd succeeds. The test probes for this with `wine cmd /c "cd /d Z:\ || exit /b 7"`. When the probe fails, it strips ` || exit /b 1` before running, and every other line stays verbatim. The test first asserts that the generated batch does contain the guard. The cd-failure branch is still covered only by the exact-content unit tests, and real Windows honours `||` for builtins. The production batch was not changed to work around the Wine bug.

**Scope.** This checks the `.bat` content only, not how cmd, Windows Terminal or PowerShell launch it. That is recorded in CCS-184, and the manual Windows desktop check stays open there.

**Files.** `src-tauri/src/platform/terminal/resume.rs` (test only) and `docs/roadmap/tickets/P5-protocol-ir-commands/CCS-184-p5-session-resume-cross-platform.md`.

**Commits.** 56c7822 has the test and the CCS-184 note. The next commit, a docs commit, records this ticket.

**Verification** (wine-9.0, Ubuntu package, default prefix):
- `cargo test --manifest-path src-tauri/Cargo.toml --lib under_wine -- --ignored`: 1 passed in about 8s.
- Mutation check: changing `call {command}` to `{command}` in `build_windows_resume_batch` makes the test fail with "batch should continue after `call` and delete itself under /c". The change was then reverted.
- `cargo test --manifest-path src-tauri/Cargo.toml --lib platform::terminal`: 36 passed, 0 failed, 3 ignored.
- `cargo test --manifest-path src-tauri/Cargo.toml --lib`: 2869 passed, 0 failed, 12 ignored.
- `cargo fmt --check`, `cargo clippy -- -D warnings` and `cargo clippy --tests -- -D warnings` are clean.

**Review follow-up.** The review added `^` to the test cwd (`proj (1)&x^y`), because an earlier bug caret-escaped characters inside the quoted `cd /d` path, and a literal `^` in the path now guards against that coming back. The review's mutation probe (dropping `call`) was confirmed to fail the test ("batch should continue after `call` and delete itself under /c") and was then reverted, so the production batch is unchanged. Re-verified after the follow-up: the wine test passes (1 passed); `cargo fmt --check`, `cargo clippy -- -D warnings`, `cargo clippy --tests -- -D warnings` are clean, and `cargo test --manifest-path src-tauri/Cargo.toml --lib` gives 2869 passed, 0 failed, 12 ignored.

**Review.** An independent review checked the commits against both acceptance criteria. The Wine test passes (1 passed, about 8s). `platform::terminal` gives 36 passed, 0 failed, 3 ignored, and the full `--lib` suite gives 2869 passed, 0 failed, 12 ignored. `cargo fmt --check` and `cargo clippy --tests -- -D warnings` are clean. As an extra mutation probe, the review made `build_windows_cwd_command_str` caret-escape `^` and `&` inside the quoted path again. The test failed on the marker CD assertion (cd stayed in the parent dir), and the probe was reverted. The only change was to the CCS-184 note, which still gave the old cwd `proj (1)&x\100%` and now says `proj (1)&x^y\100%`. No production or test code changed.
