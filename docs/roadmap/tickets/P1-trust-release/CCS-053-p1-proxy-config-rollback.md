# CCS-053: [proxy] Roll back the server and DB config when a port or address change fails to bind

**Status:** Done · **Kind:** bug · **Labels:** `bug`, `phase:P1`, `p1-high`, `area:proxy`

> **Ticket key:** `p1-proxy-config-rollback` · **Phase:** P1 · **Priority:** p1-high · **Effort:** S · **Depends on:** `p1-proxy-lifecycle-lock`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Verified finding proxy-port-change-no-rollback. update_config saves the new address/port, stops the old server, and then fails to bind (port in use, or a bad address). No proxy is left running, the DB keeps the bad port, and taken-over clients point at a closed port. Every later startup fails the same way.

## Evidence
- src-tauri/src/services/proxy.rs:3045-3082 (save, take(), stop, start with ?), 3074-3080 (persist_ephemeral failure path); server.rs:94-114; commands/proxy.rs:79-83

## Proposed change
- Start the new server before stopping the old one when the addresses differ.
- Otherwise stop, try the new config, and on failure restart from `previous` and put it back in server_guard.
- Write `previous` back to the DB on any failure, or persist only after a successful start.

## Acceptance criteria / verification
- [x] Tokio test: start on a free port, occupy a second port, and update_config to it returns Err. Afterwards get_status().running is true, get_config().listen_port is the original, and a TCP connect to the original succeeds.
- [x] `cargo test --manifest-path src-tauri/Cargo.toml update_config`

## Dependencies
p1-proxy-lifecycle-lock

## Implementation notes

**Summary.** `ProxyService::update_config` (src-tauri/src/services/proxy.rs) now delegates restarts to a new `restart_with_config` helper (called under the existing `lifecycle` lock from CCS-054):
- **Port changed:** the new server is bound first; only after a successful bind (and ephemeral-port persist) is it swapped into `self.server` and the old server stopped. A failed bind leaves the old server untouched and running.
- **Same port, address changed:** the old server must be stopped first (the port would conflict). If the new config fails to start, a server is restarted from `previous` and put back into the server slot. If stopping the old server reports an error, it is not put back (its shutdown signal was already sent, so it would be a dead instance reported as running); the new config is tried and, on failure, `previous` is restarted.
- **Any failure:** `previous` is written back to the DB, so later startups and the Live takeover keep using the working address/port. Takeover Live rewrites only happen after a successful restart, as before.

**Files.** `src-tauri/src/services/proxy.rs` (implementation + 2 tokio tests).

**Commits.** `2bd2dfb` (code + tests); this ticket-file update in a follow-up docs commit.

**Tests added.**
- `update_config_to_occupied_port_keeps_original_server_and_config`: starts on a free port, occupies a second port, and `update_config` to it returns Err. Then `get_status().running` is true with the original port, `get_config().listen_port` is the original, and a TCP connect to the original succeeds.
- `update_config_to_invalid_address_restores_previous_server`: a same-port change to an invalid address returns Err. The previous server is restored on the original port and the DB keeps the old address/port.

**Verification.**
- `cargo test --manifest-path src-tauri/Cargo.toml --lib update_config`: 2 passed. With the old restart logic swapped back in temporarily, both tests FAILED (regression confirmed).
- `cargo fmt --manifest-path src-tauri/Cargo.toml -- --check`: clean.
- `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings`: clean.
- `cargo test --manifest-path src-tauri/Cargo.toml --lib`: 2111 passed, 0 failed, 3 ignored.

**Review.** Verified each acceptance criterion against `2bd2dfb`; both tests assert the fix (running status, original port in status and DB, TCP connect). Follow-up fixes in `restart_with_config`:
- Same-port path: when `stop()` on the old server returned an error, the implementation put the already-signalled instance back into the server slot, so `is_running()` stayed true for a server that was shutting down (and inconsistent with `ProxyService::stop`, which drops it). It now logs the error and continues with the new config / `previous` fallback.
- Port-change path: if the new server binds but persisting the ephemeral port fails, `ProxyServer::start` has already updated the global `http_client` proxy port used for self-loop detection; it is now reset to the still-running old server's port.
- Re-verified: fmt/clippy clean; `cargo test --lib` 2111 passed / 0 failed; full `cargo test --no-fail-fast` 2267 passed / 0 failed.
