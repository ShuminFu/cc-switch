# CCS-248: [e2e] Automate the Linux session-resume check under xvfb with a fake claude

**Status:** Open · **Kind:** chore · **Labels:** `chore`, `phase:P5`, `p3-low`, `area:e2e`

> **Ticket key:** `p5-e2e-session-resume-linux` · **Phase:** P5 · **Priority:** p3-low · **Effort:** M
> Plan: [next-phase-plan.md](../../next-phase-plan.md)
> Found during the Partial-ticket triage at d962272 (CCS-184, CCS-006). Source tickets: [CCS-184](../P5-protocol-ir-commands/CCS-184-p5-session-resume-cross-platform.md), [CCS-006](../P5-protocol-ir-commands/CCS-006-epic-p5.md).

## Problem
CCS-184's Linux manual resume check (Resume opens a terminal in the session cwd running `claude --resume <id>`, the temp script is deleted, the window stays open) can be automated: Xvfb, xvfb-run, xterm, WebKitWebDriver, tauri-driver and a debug binary are all available, and `e2e/wdio.conf.mjs` already isolates HOME (`CC_SWITCH_E2E_HOME`).

## Proposed change
- Add `e2e/specs/session-resume.e2e.mjs`. Before launch: write a fake Claude session JSONL under `$HOME/.claude/projects/<encoded-dir>/<id>.jsonl` whose cwd is a temp dir with a space and parentheses in its name; put a fake `claude` first on the app's PATH (via `appEnv`) that records `pwd` and its args to a marker file; seed `preferred_terminal = "xterm"` (`linux.rs:69` falls back to `-e`).
- Spec: open Sessions, select the session, click Resume, wait for the marker. Assert cwd == session dir, args exactly `--resume <id>`, `cc_switch_resume_*.sh` deleted, no error toast, xterm still alive (exec $SHELL).
- Non-GUI alternative: an `#[ignore]` Rust test calling `platform::terminal::launch_resume_terminal` under xvfb-run with the same fake claude.

## Acceptance criteria / verification
- [ ] `xvfb-run -a pnpm --dir e2e test` passes with the new spec (after `pnpm tauri build --debug --no-bundle`).
- [ ] Result recorded in CCS-184 as the automated Linux check.

## Dependencies
None.
