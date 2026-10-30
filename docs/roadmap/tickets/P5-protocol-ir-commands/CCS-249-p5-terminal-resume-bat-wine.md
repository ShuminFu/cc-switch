# CCS-249: [terminal] Run the generated Windows resume .bat under Wine cmd

**Status:** Open · **Kind:** chore · **Labels:** `chore`, `phase:P5`, `p3-low`, `area:terminal`

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
- [ ] The Wine run passes all four assertions.
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml --lib platform::terminal` still passes.

## Dependencies
None.
