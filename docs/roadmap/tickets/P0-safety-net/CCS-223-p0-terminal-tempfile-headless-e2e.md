# CCS-223: [terminal] Headless Linux end-to-end check of private terminal temp files, plus temp-dir selection tests

**Status:** Open · **Kind:** security · **Labels:** `security`, `phase:P0`, `p2-medium`, `area:terminal`

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
- [ ] `xvfb-run -a cargo test --manifest-path src-tauri/Cargo.toml --lib -- --ignored ccs036_e2e` passes; output recorded in CCS-036 as the Linux verification.
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml --lib platform::terminal` passes with the new temp-dir tests.
- [ ] `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings` is clean.

## Dependencies
None.
