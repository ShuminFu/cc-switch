# CCS-184: [sessions] Support session resume on Windows and Linux

**Status:** Open · **Kind:** feature · **Labels:** `feature`, `phase:P5`, `p3-low`, `area:sessions`

> **Ticket key:** `p5-session-resume-cross-platform` · **Phase:** P5 · **Priority:** p3-low · **Effort:** M · **Depends on:** `p5-misc-split-terminal`, `p1-ipc-session-terminal`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Session resume is macOS-only, although Linux and Windows launchers exist.

## Proposed change
Use platform/terminal for resume on all 3 OSes through the backend-built command from p1-ipc-session-terminal.

## Acceptance criteria / verification
- [ ] Per-OS unit tests of the generated launch script
- [ ] Manual resume on Windows and Linux
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml terminal`

## Dependencies
p5-misc-split-terminal, p1-ipc-session-terminal
