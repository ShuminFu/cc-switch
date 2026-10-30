# CCS-038: [usage] Move session-log sync off tokio workers and off the main thread

**Status:** Partial · **Kind:** bug · **Labels:** `bug`, `phase:P0`, `p1-high`, `area:usage`

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
- [x] `cargo check` and `cargo test --manifest-path src-tauri/Cargo.toml session_usage`
- [x] A debug log shows the command no longer runs on the `main` thread
- [ ] Manual: with many large .jsonl files, the window stays responsive during a manual sync

## Dependencies
None.

## Implementation notes

**Status rationale:** code and automated checks are done; the manual
responsiveness check with a large `~/.claude/projects` needs a desktop session
and has not been run here.

### Summary
- `services/session_usage.rs`: new `sync_all_session_usage(&Database)` (the
  Claude + Codex + Gemini + OpenCode aggregation that used to live in the
  command), `run_sync_blocking(label, f)` (runs `f` via
  `tauri::async_runtime::spawn_blocking`, emits a `debug!` log with the executing
  thread name, maps a join error to `AppError::Message`), and
  `sync_all_session_usage_blocking(Arc<Database>)`.
- `commands/usage.rs`: `sync_session_usage` is now `async` and delegates to
  `sync_all_session_usage_blocking(state.db.clone())`, so it no longer runs on the
  main thread. The frontend `invoke("sync_session_usage")` is unchanged.
- `lib.rs`: the startup backfill and each startup/periodic sync step in the 60 s
  loop are run via `run_sync_blocking`, so they no longer block a tokio worker.

### Files
- `src-tauri/src/services/session_usage.rs`
- `src-tauri/src/commands/usage.rs`
- `src-tauri/src/lib.rs`

### Commits
- `833fb94` fix(usage): run session-log sync on the blocking pool (CCS-038)

### Verification
- `cargo fmt --manifest-path src-tauri/Cargo.toml --check`: clean.
- `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings`: clean.
- `cargo test --manifest-path src-tauri/Cargo.toml --lib session_usage`: 41 passed, 0 failed.
- `cargo test --manifest-path src-tauri/Cargo.toml --lib`: 2020 passed, 0 failed, 2 ignored.
- New regression tests in `services::session_usage::tests`:
  - `test_run_sync_blocking_runs_off_caller_thread`: the closure must run on a
    different thread from the caller.
  - `test_run_sync_blocking_does_not_stall_async_workers`: on a single-threaded
    tokio runtime, a blocking step waits for a signal from a concurrent async
    task. This only finishes if the step does not hold the async thread.
  - `test_run_sync_blocking_propagates_errors`.
  To show these catch the bug, `run_sync_blocking` was temporarily changed to
  call `f()` inline (the old behaviour). The first two tests then failed
  (thread-id assertion; 5 s recv timeout) and passed again once the fix was
  restored.
- Thread debug log: `[SESSION-SYNC] Session usage sync running on thread "..."`
  is emitted at debug level. The first test checks the same thing.
- Not run: the manual UI responsiveness check with many large `.jsonl` files
  (needs a desktop session).

### Remaining after triage

- Blocked: nothing; every remaining item is codable.
- Follow-up [CCS-221](../P0-safety-net/CCS-221-p0-docs-ticket-evidence-refresh.md): Tick already-verified acceptance boxes and refresh stale evidence in Partial tickets.
- Follow-up [CCS-224](../P0-safety-net/CCS-224-p0-usage-session-sync-responsiveness.md): Prove the UI stays responsive during a large session-log sync with a headless e2e spec.

### Evidence refresh (CCS-221)

The 60 s loop moved out of `lib.rs`: it is now `session_usage_sync_loop` in
`src-tauri/src/app_shell/bootstrap.rs` (lines 1069-1152 at f13a403; the
periodic `loop` starts at line 1125). Each step still goes through
`services::session_usage::run_sync_blocking`, whose
`[SESSION-SYNC] {label} running on thread {:?}` debug log is at
`services/session_usage.rs:841`. The `sync_session_usage` command is now in
`commands/usage.rs:204` (still `async`).

Re-run on the lane2/followups tree at f13a403 (Linux):
- `cargo test --manifest-path src-tauri/Cargo.toml --lib session_usage` → 56
  passed, 0 failed, including `test_run_sync_blocking_runs_off_caller_thread`
  (this also proves the build, so it covers `cargo check`).
- `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings`
  → clean.
- The test box and the debug-log box are ticked. The manual responsiveness check
  stays open (CCS-224 covers it headlessly).
