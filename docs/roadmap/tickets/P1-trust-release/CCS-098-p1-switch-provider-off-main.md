# CCS-098: [providers] Run the switch_provider command off the main thread

**Status:** Done · **Kind:** bug · **Labels:** `bug`, `phase:P1`, `p3-low`, `area:providers`

> **Ticket key:** `p1-switch-provider-off-main` · **Phase:** P1 · **Priority:** p3-low · **Effort:** S
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Verified finding backend-switch-provider-block-on, partially real. switch_provider is a sync command, so it runs on the main thread and block_on's the per-app switch lock and file I/O. A concurrent takeover or hot switch stalls the UI briefly. The tray is unaffected, because it already runs in spawn_blocking.

## Evidence
- src-tauri/src/commands/provider.rs:102-110; services/provider/mod.rs:2530-2537; services/proxy.rs:510-515, 733, 1783, 2289, 2400

## Proposed change
Use `#[tauri::command(async)]`, or an async fn calling spawn_blocking(ProviderService::switch). Leave tray.rs as is.

## Acceptance criteria / verification
- [x] Test: hold lock_switch_for_app("claude") and call switch on another task; it does not block the calling thread, and completes after release
- [x] `cargo test --manifest-path src-tauri/Cargo.toml provider` and clippy

## Dependencies
None.

## Implementation notes

**Summary.** `switch_provider` is now an `async` Tauri command. It takes the `AppHandle` and hands `ProviderService::switch` to `tauri::async_runtime::spawn_blocking` through a new helper, `run_switch_off_main` (`pub(crate)`, in `commands/provider.rs`). A join error is mapped to `AppError::Message`. The per-app switch-lock `block_on` and the live-file I/O now run on the blocking pool, not on the main thread. The command's name, arguments and result type are unchanged, so the frontend `invoke` calls need no edits. `tray.rs` is untouched (it already uses `spawn_blocking`).

**Files**
- `src-tauri/src/commands/provider.rs`: async `switch_provider` and the `run_switch_off_main` helper
- `src-tauri/src/services/provider/mod.rs`: regression test `switch_command_waits_for_switch_lock_off_the_calling_thread`

**Commits:** `8d5921e` (code + test); this ticket update follows in a separate docs commit.

**Regression test.** The test holds `lock_switch_for_app("claude")` and calls the helper from a single-threaded `#[tokio::test]` task. It checks three things:
- A 300 ms `timeout` on the switch future elapses, so the calling task is not blocked and the switch is still waiting.
- The current provider is still `a` while the lock is held.
- After the guard is dropped, the switch completes: the current provider becomes `b` and the live `~/.claude/settings.json` has `token-b`.

To show the test catches the old behaviour, I temporarily changed the helper to call `switch()` inline (the old sync behaviour). The test then deadlocked and was killed by `timeout 600` (exit 124). After restoring the fix it passes.

**Verification**
- `cargo fmt --manifest-path src-tauri/Cargo.toml -- --check`: clean
- `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings`: clean
- `cargo test --manifest-path src-tauri/Cargo.toml --lib switch_command_waits`: 1 passed
- `cargo test --manifest-path src-tauri/Cargo.toml --lib`: 2271 passed, 0 failed, 2 ignored
- `cargo test --manifest-path src-tauri/Cargo.toml provider`: lib 940 passed; integration suites all pass (provider_commands 4, provider_service 17, proxy_e2e 3, others as filtered), 0 failed
