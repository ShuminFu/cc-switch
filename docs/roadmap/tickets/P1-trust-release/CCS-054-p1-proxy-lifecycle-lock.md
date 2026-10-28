# CCS-054: [proxy] Serialize proxy start/stop with a lifecycle mutex and stop holding the server lock across shutdown

**Status:** Open · **Kind:** bug · **Labels:** `bug`, `phase:P1`, `p2-medium`, `area:proxy`

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
- [ ] Test: `tokio::join!(svc.start(), svc.start())` with port 0 makes both report the same port, which equals the DB port and accepts a TCP connect (inject yield_now to make the race reliable)
- [ ] Test: during a slow stop, `timeout(100ms, svc.is_running())` returns
- [ ] `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings -W clippy::significant_drop_in_scrutinee` is clean for :1262

## Dependencies
None.
