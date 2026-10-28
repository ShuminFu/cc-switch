# CCS-099: [providers] Take the per-app switch lock for every app type

**Status:** Open · **Kind:** bug · **Labels:** `bug`, `phase:P1`, `p3-low`, `area:providers`

> **Ticket key:** `p1-switch-lock-all-apps` · **Phase:** P1 · **Priority:** p3-low · **Effort:** S · **Depends on:** `p1-switch-provider-off-main`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Verified finding backend-switch-lock-partial, partially real. Switches for OpenCode (including OMO and OMO-Slim), ClaudeDesktop, OpenClaw and Hermes take no lock. A deeplink auto-switch racing a UI or profile switch can lose an update.

## Evidence
- src-tauri/src/services/provider/mod.rs:2510-2539; deeplink/provider.rs:132; services/profile.rs:384

## Proposed change
Take lock_switch_for_app(app_type) at the top of switch, before the OMO and ClaudeDesktop early returns. Check that switch_normal does not take the lock again, because a tokio Mutex is not reentrant.

## Acceptance criteria / verification
- [ ] Test: two threads switch OpenCode a and b; the DB current provider equals the live opencode config
- [ ] The services/proxy.rs:6371 tests still pass (no deadlock)
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml switch_concurrent`

## Dependencies
p1-switch-provider-off-main
