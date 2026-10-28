# CCS-038: [usage] Move session-log sync off tokio workers and off the main thread

**Status:** Open · **Kind:** bug · **Labels:** `bug`, `phase:P0`, `p1-high`, `area:usage`

> **Ticket key:** `p0-hotfix-session-sync-blocking` · **Phase:** P0 · **Priority:** p1-high · **Effort:** S
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Verified finding backend-session-sync-blocking. The 60 s periodic sync runs blocking file scans and std::Mutex DB work inside an async task. The sync_session_usage command is synchronous, so it runs on the main thread and freezes the UI on large ~/.claude/projects.

## Evidence
- src-tauri/src/lib.rs:1092-1149; services/session_usage.rs:61-91,182-222; commands/usage.rs:277-279; frontend DataSourceBar.tsx:38

## Proposed change
- Wrap each sync step in tauri::async_runtime::spawn_blocking.
- Make sync_session_usage `async` with spawn_blocking on a cloned Arc<Database>.

## Acceptance criteria / verification
- [ ] `cargo check` and `cargo test --manifest-path src-tauri/Cargo.toml session_usage`
- [ ] A debug log shows the command no longer runs on the `main` thread
- [ ] Manual: with many large .jsonl files, the window stays responsive during a manual sync

## Dependencies
None.
