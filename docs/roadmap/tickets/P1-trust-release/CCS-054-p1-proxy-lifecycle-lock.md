# CCS-054: [proxy] Serialize proxy start/stop with a lifecycle mutex and stop holding the server lock across shutdown

**Status:** Done · **Kind:** bug · **Labels:** `bug`, `phase:P1`, `p2-medium`, `area:proxy`

> **Ticket key:** `p1-proxy-lifecycle-lock` · **Phase:** P1 · **Priority:** p2-medium · **Effort:** S
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Covers verified findings proxy-start-race and proxy-server-write-guard-stop. Both are fixed by one lifecycle mutex.
- Two concurrent start() calls with port 0 both pass the not-running check and bind different ports. The persisted port, the global proxy port or the returned ProxyServerInfo can then name the dropped server.
- stop() holds the self.server write guard across server.stop() (up to 5 s) and the DB writes, because of edition-2021 if-let temporaries. That blocks is_running and status polling.

## Evidence
- src-tauri/src/services/proxy.rs:518-572 (start check-then-act, :542/:569), :1262 `if let Some(server) = self.server.write().await.take()`; src-tauri/Cargo.toml:8 edition 2021; server.rs:225-255

## Proposed change
- Add `lifecycle: tokio::sync::Mutex<()>`. start, stop, stop_with_restore and update_config hold it for the whole sequence and re-check state after acquiring it.
- Take the server out in its own statement so the RwLock guard drops before awaiting stop().

## Acceptance criteria / verification
- [x] Test: `tokio::join!(svc.start(), svc.start())` with port 0 makes both report the same port, which equals the DB port and accepts a TCP connect (inject yield_now to make the race reliable)
- [x] Test: during a slow stop, `timeout(100ms, svc.is_running())` returns
- [x] `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings -W clippy::significant_drop_in_scrutinee` is clean for :1262

## Dependencies
None.

## Implementation notes
**Summary.** `ProxyService` gained a `lifecycle: Arc<tokio::sync::Mutex<()>>`. The public `start`/`stop` acquire it and delegate to `start_locked`/`stop_locked`. `stop_with_restore` and `stop_with_restore_keep_state` hold it for their whole sequence and call `stop_locked`. `update_config` holds it from the first DB read through the restart and the Live-config proxy-address resync. `start_locked` checks for a running server only after the lock is taken, so the second of two concurrent starts sees the first server and returns its port. `stop_locked` and the `update_config` restart path now take the server out in a separate `let` statement, so the `server` RwLock write guard is dropped before `server.stop()` (up to 5 s) and the DB writes run. `ProxyServer::stop` (server.rs) does the same for its `shutdown_tx`/`server_handle` slots. The `apply_runtime_config` path now takes a short read guard instead of holding the write guard. Two test-only hooks were added: a `yield_now` after the not-running check, which makes the start race reproducible, and a `stop_delay_ms_for_test` that simulates a slow stop.

**Files.** `src-tauri/src/services/proxy.rs`, `src-tauri/src/proxy/server.rs`.

**Commits.** a89f3ba (code + tests), plus this ticket-notes commit.

**Tests** (in `services::proxy::tests`):
- `concurrent_start_with_ephemeral_port_binds_a_single_server`: `tokio::join!(svc.start(), svc.start())` with port 0. Both calls report the same non-zero port, which matches the persisted DB `listen_port` and `get_status().port`, and a TCP connect to it succeeds.
- `is_running_is_not_blocked_by_slow_stop`: while a 1 s slow stop is running, `timeout(100ms, is_running())` and `timeout(100ms, get_status())` both return.
- Both tests were confirmed to FAIL with the lock and the separate-statement take reverted, and to PASS with the fix.

**Verification.**
- `cargo fmt --check`: clean.
- `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings`: clean.
- `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings -W clippy::significant_drop_in_scrutinee`: no hits in `services/proxy.rs` (the former :1262 is gone) and none in `ProxyServer::stop`. Hits remain only in code this ticket does not cover: `proxy/circuit_breaker.rs:237,262` and `proxy/server.rs:289` (`get_status`, a read guard over a sync read).
- `cargo test --manifest-path src-tauri/Cargo.toml --no-fail-fast`: 2265 passed / 0 failed.
