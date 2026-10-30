# CCS-099: [providers] Take the per-app switch lock for every app type

**Status:** Done · **Kind:** bug · **Labels:** `bug`, `phase:P1`, `p3-low`, `area:providers`

> **Ticket key:** `p1-switch-lock-all-apps` · **Phase:** P1 · **Priority:** p3-low · **Effort:** S · **Depends on:** `p1-switch-provider-off-main`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Verified finding backend-switch-lock-partial, partially real. Switches for OpenCode (including OMO and OMO-Slim), ClaudeDesktop, OpenClaw and Hermes take no lock. A deeplink auto-switch racing a UI or profile switch can lose an update.

## Evidence
- src-tauri/src/services/provider/mod.rs:2510-2539; deeplink/provider.rs:132; services/profile.rs:384

## Proposed change
Take lock_switch_for_app(app_type) at the top of switch, before the OMO and ClaudeDesktop early returns. Check that switch_normal does not take the lock again, because a tokio Mutex is not reentrant.

## Acceptance criteria / verification
- [x] Test: two threads switch OpenCode a and b; the DB current provider equals the live opencode config
- [x] The services/proxy.rs:6371 tests still pass (no deadlock)
- [x] `cargo test --manifest-path src-tauri/Cargo.toml switch_concurrent`

## Dependencies
p1-switch-provider-off-main

## Implementation notes

**Summary.** `ProviderService::switch` (`src-tauri/src/services/provider/mod.rs`) now acquires
`proxy_service.lock_switch_for_app(app_type)` as its first statement, for every app type, before
the provider snapshot is read and before the OMO / OMO Slim / ClaudeDesktop early returns. The old
Claude/Codex/Gemini/GrokBuild-only guard was removed. Nothing below it re-locks: `switch_normal`,
`OmoService::write_config_to_file`, `write_live_with_common_config`, `McpService::sync_enabled_for_app`
and `hot_switch_provider_inner` take no switch lock (the locking wrappers `hot_switch_provider` /
`update_live_backup_from_provider` are not reached from `switch`). Callers (`commands/provider.rs`
via `run_switch_off_main`, `tray.rs`, `deeplink/provider.rs`, `services/profile.rs`) do not hold the
lock when calling `switch`, so no deadlock.

**Files.** `src-tauri/src/services/provider/mod.rs` (fix + tests), this ticket.

**Commits.** `d761d59` fix(providers): take the per-app switch lock for every app type (CCS-099); ticket notes in the follow-up docs commit.

**Tests added** (`services::provider::tests`):
- `switch_concurrent_opencode_waits_for_per_app_lock` — holds the `opencode` lock and asserts an OMO
  switch and a plain additive OpenCode switch both block until it is released, then checks DB current
  OMO == live OMO config and that `opencode.json` holds the plain provider. Verified to FAIL with the
  old app-type-restricted guard and pass with the fix.
- `switch_concurrent_opencode_omo_keeps_db_and_live_in_sync` — two threads switch OpenCode OMO
  a/b 25 times each; afterwards the DB current OMO provider equals the live OMO config. (Stress test;
  the race window is small, so it is not a guaranteed pre-fix failure — the deterministic test above is
  the regression test.)

**Verification.**
- `cargo fmt --check` — clean
- `cargo clippy -- -D warnings` — clean
- `cargo test --lib switch_concurrent` — 2 passed
- `cargo test --lib` — 2273 passed, 0 failed (includes the `services::proxy` switch/takeover tests; no deadlock)
- `cargo test --no-fail-fast` — 2439 passed, 0 failed
