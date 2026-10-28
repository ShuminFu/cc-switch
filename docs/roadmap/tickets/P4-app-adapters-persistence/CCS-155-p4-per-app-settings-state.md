# CCS-155: [settings] Use keyed per-app maps with old-key dual-write, and migrate profiles and localStorage

**Status:** Open · **Kind:** refactor · **Labels:** `refactor`, `phase:P4`, `p2-medium`, `area:settings`

> **Ticket key:** `p4-per-app-settings-state` · **Phase:** P4 · **Priority:** p2-medium · **Effort:** M · **Depends on:** `p2-app-registry`, `p1-settings-save-race`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Per-app settings are flat keys, so every new app needs new keys. The critique adds three points:
- reading the old keys is not enough; without also writing them, a downgrade loses current providers, visible apps and overrides
- the profiles table and localStorage are keyed by app id

## Evidence
- settings.rs current_provider_*, visible apps, config dirs; schema.rs:314 profiles; tests/profile_roundtrip.rs; App.tsx STORAGE_KEY, useCodexCommonConfig, useGeminiCommonConfig

## Proposed change
- Serialize CommonConfigSnippets, VisibleApps, current_providers and config_dirs as maps, with serde reading the old flat keys and also writing them for one release.
- Map profiles and localStorage keys through the registry aliases.

## Acceptance criteria / verification
- [ ] Test: a new-version settings.json loaded by the old struct (a copy kept in tests) still finds the current providers
- [ ] profile_roundtrip passes
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml settings` and `pnpm test:unit`

## Dependencies
p2-app-registry, p1-settings-save-race
