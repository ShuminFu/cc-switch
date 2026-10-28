# CCS-053: [proxy] Roll back the server and DB config when a port or address change fails to bind

**Status:** Open · **Kind:** bug · **Labels:** `bug`, `phase:P1`, `p1-high`, `area:proxy`

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
- [ ] Tokio test: start on a free port, occupy a second port, and update_config to it returns Err. Afterwards get_status().running is true, get_config().listen_port is the original, and a TCP connect to the original succeeds.
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml update_config`

## Dependencies
p1-proxy-lifecycle-lock
