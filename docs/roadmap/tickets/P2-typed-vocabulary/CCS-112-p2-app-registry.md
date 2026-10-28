# CCS-112: [apps] Add the AppDescriptor registry and replace hard-coded app lists

**Status:** Open · **Kind:** refactor · **Labels:** `refactor`, `phase:P2`, `p1-high`, `area:apps`

> **Ticket key:** `p2-app-registry` · **Phase:** P2 · **Priority:** p1-high · **Effort:** M · **Depends on:** `p0-config-writer-snapshots`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
About 15 hand-kept app lists drift apart. Adding Grok Build touched 112 files.

## Evidence
- lib.rs:1754, lib.rs:788-819, services/proxy.rs:1308, tray.rs:157; app_config.rs:28-35

## Proposed change
- Add apps/registry.rs with `static APPS: &[AppDescriptor]` (slug, aliases, label key, SyncMode, AppCaps bitflags, proxy_route, input_token_semantics, default_visible).
- AppType::as_str, FromStr, is_additive_mode and all() delegate to it.
- Replace the listed sites with APPS.filter(caps).

## Acceptance criteria / verification
- [ ] No hard-coded app list at the 4 sites (grep check in CI)
- [ ] Config-writer snapshots, proxy_e2e and migrations are unchanged
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml`

## Dependencies
p0-config-writer-snapshots
