# CCS-096: [usage] Advance the session-log offset only past complete newline-terminated lines

**Status:** Open · **Kind:** bug · **Labels:** `bug`, `phase:P1`, `p2-medium`, `area:usage`

> **Ticket key:** `p1-session-sync-partial-line` · **Phase:** P1 · **Priority:** p2-medium · **Effort:** S
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Verified finding backend-session-usage-partial-line. A trailing line that is still being written fails to parse but is still counted in line_offset. It is skipped forever once complete.

## Evidence
- src-tauri/src/services/session_usage.rs:181-355 (~207 line_offset += 1, ~223 Err continue, ~352 update_sync_state)

## Proposed change
- Read with read_until(b'\n') and count only lines that end in '\n'.
- Consider a byte offset plus seek.
- Apply the same change to the other session_usage_* parsers that share get_sync_state.

## Acceptance criteria / verification
- [ ] Test: one complete line plus a truncated second line gives 1 import; complete it, bump the mtime, and the second is imported
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml session_usage`

## Dependencies
None.
