# CCS-223: [terminal] Headless Linux end-to-end check of private terminal temp files, plus temp-dir selection tests

**Status:** Done · **Kind:** security · **Labels:** `security`, `phase:P0`, `p2-medium`, `area:terminal`

> **Ticket key:** `p0-terminal-tempfile-headless-e2e` · **Phase:** P0 · **Priority:** p2-medium · **Effort:** M
> Plan: [next-phase-plan.md](../../next-phase-plan.md)
> Found during the Partial-ticket triage at d962272 (CCS-036, CCS-001). Source tickets: [CCS-036](../P0-safety-net/CCS-036-p0-hotfix-terminal-tempfiles.md), [CCS-001](../P0-safety-net/CCS-001-epic-p0.md).

## Problem
CCS-036's only unmet Linux item is a manual desktop check (`ls -l` shows `-rw-------`, the file is gone after claude exits, launching A then B quickly gives each its own config path). That needs no real desktop: `/usr/bin/xterm`, `xvfb-run` and `Xvfb` are installed. Separately, `private_launch_temp_dir` (`src-tauri/src/platform/terminal/tempfiles.rs:32`) has no tests for its XDG_RUNTIME_DIR / `<app_config_dir>/tmp` selection or for tightening a 0755 dir to 0700.

## Proposed change
- Add an `#[ignore]`, Linux-only test (`ccs036_e2e`) or a `scripts/verify-ccs036.sh` wrapper:
  1. `CC_SWITCH_TEST_HOME` = tempdir; `XDG_RUNTIME_DIR` = a 0700 tempdir (and one run with it unset, for the fallback).
  2. `preferred_terminal = "xterm"` (`linux.rs:68` gives unknown terminals `-e`).
  3. A stub `claude` first on PATH parses `--settings <path>`, logs the path, `stat -c %a` and `ls -l`, sleeps briefly and exits; `SHELL=/bin/sh`.
  4. Call `launch_terminal_with_env(env_A, "provA", None)` then `launch_terminal_with_env(env_B, "provB", None)` at once.
  5. Assert distinct paths with each provider's env, mode 600, and that neither config nor `cc_switch_launcher_*.sh` remains afterwards.
- Add Linux unit tests for `private_launch_temp_dir` (serialised with the env-var lock): absolute XDG dir is used; relative/missing falls back to `get_app_config_dir()/tmp` with mode 0700; a pre-existing 0755 dir is tightened to 0700.

## Acceptance criteria / verification
- [x] `xvfb-run -a cargo test --manifest-path src-tauri/Cargo.toml --lib -- --ignored ccs036_e2e` passes; output recorded in CCS-036 as the Linux verification.
- [x] `cargo test --manifest-path src-tauri/Cargo.toml --lib platform::terminal` passes with the new temp-dir tests.
- [x] `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings` is clean.

## Dependencies
None.

## Implementation notes

**Summary.** Test-only change; the production code already behaved correctly, so
these tests guard it against regressions rather than reproduce a bug.
- `tests::launch_temp_dir` (Linux, `#[serial_test::serial]` because
  `XDG_RUNTIME_DIR` is process-wide; the home dir is injected per thread via
  `paths::scoped`, and an `EnvVarGuard` restores env vars):
  `uses_absolute_xdg_runtime_dir` (the fallback dir is not created),
  `missing_xdg_runtime_dir_falls_back_to_private_app_config_tmp`
  (`get_app_config_dir()/tmp`, mode 0700),
  `relative_or_nonexistent_xdg_runtime_dir_falls_back` (relative, empty and
  non-existent values), and
  `existing_world_readable_fallback_dir_is_tightened_to_0700`.
- `tests::ccs036_e2e` (Linux, `#[ignore]`): `ccs036_e2e_xdg_runtime_dir` (0700
  `XDG_RUNTIME_DIR`) and `ccs036_e2e_app_config_fallback` (unset). Each one sets
  `preferred_terminal = "xterm"` in a scoped settings store, puts a stub `claude`
  first on `PATH`, sets `SHELL=/bin/sh` and `ENV=<rc that exits>` so the final
  interactive `exec sh` closes the xterm, then calls
  `launch_terminal_with_env(env_A, "provA", None)` and `(env_B, "provB", None)`
  back to back. The stub logs the `--settings` path, `stat -c %a`, `ls -l`,
  the launcher script modes and a copy of the config. The test asserts that
  the paths are distinct and in the selected temp dir, that each has the
  `claude_<provider>_` prefix and that provider's token, mode 600 /
  `-rw-------`, launchers 0700, and that no `claude_*` or
  `cc_switch_launcher_*` file remains. A drop guard runs `pkill -f <temp dir>`
  in case an xterm is left behind.
- The test isolates the home dir with `paths::scoped(Paths::with_home(..))`,
  which is what `CC_SWITCH_TEST_HOME` resolves to, without changing the
  process-wide variable.
- The output is recorded in CCS-036 under "Linux verification (CCS-223)", and
  CCS-036's Linux manual box is ticked.

**Files.** `src-tauri/src/platform/terminal/tempfiles.rs`;
`docs/roadmap/tickets/P0-safety-net/CCS-036-p0-hotfix-terminal-tempfiles.md`; this ticket.

**Commits.** a2ec202 (tests); the follow-up docs commit records these notes.

**Verification (Linux).**
- `xvfb-run -a cargo test --manifest-path src-tauri/Cargo.toml --lib -- --ignored ccs036_e2e`
  → 2 passed, 0 failed (output in CCS-036).
- `cargo test --manifest-path src-tauri/Cargo.toml --lib platform::terminal` → 28 passed,
  0 failed, 2 ignored (the e2e tests).
- `cargo test --manifest-path src-tauri/Cargo.toml --lib` → 2854 passed, 0 failed, 5 ignored.
- `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings` → clean;
  `cargo fmt --check` → clean.
- Environment note: the lane2 `target/debug` had 503 fingerprints whose rlibs had been
  pruned (link errors such as "cannot open libserde_spanned-*.rlib"). Only those stale
  `.fingerprint` entries were removed so cargo rebuilt the units; `cargo clean` was not used.
