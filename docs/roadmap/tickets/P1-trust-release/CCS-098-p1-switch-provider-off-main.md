# CCS-098: [providers] Run the switch_provider command off the main thread

**Status:** Open · **Kind:** bug · **Labels:** `bug`, `phase:P1`, `p3-low`, `area:providers`

> **Ticket key:** `p1-switch-provider-off-main` · **Phase:** P1 · **Priority:** p3-low · **Effort:** S
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Verified finding backend-switch-provider-block-on, partially real. switch_provider is a sync command, so it runs on the main thread and block_on's the per-app switch lock and file I/O. A concurrent takeover or hot switch stalls the UI briefly. The tray is unaffected, because it already runs in spawn_blocking.

## Evidence
- src-tauri/src/commands/provider.rs:102-110; services/provider/mod.rs:2530-2537; services/proxy.rs:510-515, 733, 1783, 2289, 2400

## Proposed change
Use `#[tauri::command(async)]`, or an async fn calling spawn_blocking(ProviderService::switch). Leave tray.rs as is.

## Acceptance criteria / verification
- [ ] Test: hold lock_switch_for_app("claude") and call switch on another task; it does not block the calling thread, and completes after release
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml provider` and clippy

## Dependencies
None.
