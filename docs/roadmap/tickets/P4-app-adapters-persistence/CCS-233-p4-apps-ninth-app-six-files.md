# CCS-233: [apps] Make a 9th app touch 6 or fewer backend files

**Status:** Open · **Kind:** refactor · **Labels:** `refactor`, `phase:P4`, `p2-medium`, `area:apps`

> **Ticket key:** `p4-apps-ninth-app-six-files` · **Phase:** P4 · **Priority:** p2-medium · **Effort:** M · **Depends on:** `p4-apps-budget-ratchet`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)
> Found during the Partial-ticket triage at d962272 (CCS-150, CCS-005, CCS-149). Source tickets: [CCS-150](../P4-app-adapters-persistence/CCS-150-p4-adapter-conformance.md), [CCS-005](../P4-app-adapters-persistence/CCS-005-epic-p4.md), [CCS-149](../P4-app-adapters-persistence/CCS-149-p4-adapter-additive.md).

## Problem
The CCS-150 / P4 criterion "dummy app touches ≤ 6 backend files" measures 9: app_config.rs (4 exhaustive matches), apps/registry.rs, apps/mod.rs, the new apps/<app>/mod.rs, plus deeplink/preview.rs (2), deeplink/provider.rs (1), paths.rs (1), proxy/providers/mod.rs (2), services/config.rs (1). The 4 fixed files are always touched, so at least 3 of the other 5 must stop matching exhaustively on `AppType`.

## Proposed change
- `src-tauri/src/paths.rs:88-98` `default_app_dir`: add a home-relative default dir (plus a special-case fn for ClaudeDesktop/Hermes) to the registry descriptor or an adapter hook, and dispatch through `APPS`/`apps::adapter(app)` without a cycle (adapters use paths). `legacy_default_app_dir` (~192) can keep the known variants plus `_ => None`; confirm whether it is test-only.
- `src-tauri/src/services/config.rs:122-140`: additive apps and ClaudeDesktop return early by `SyncMode`; switch apps call `adapter.write_provider(&provider)` or a new `sync_live_from_config` hook for the Codex/Claude/Gemini specifics (or keep explicit arms with a `_ =>` catch-all).
- `src-tauri/src/deeplink/preview.rs:342` `provider_live_files` and `:362` `mcp_live_file`: add `AppAdapter::live_files()` and use `mcp().map(|m| m.live_file())`.
- `src-tauri/src/deeplink/provider.rs:172-180`: add `AppAdapter::deeplink_settings(request)`, or a `_ => build_additive_app_settings(request)` arm.
- `proxy/providers/mod.rs` is handled in CCS-236 if these four files are not enough.
- Lower `NEW_APP_BACKEND_FILE_BUDGET` to the measured value (target 6).

## Acceptance criteria / verification
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml --test app_adapter_conformance new_app_backend_touch_points -- --nocapture` prints ≤ 6 files; the budget equals the measured count.
- [ ] The full conformance suite (13 tests), `cargo test --lib deeplink`, `cargo test --lib paths`, `cargo test --lib services::config` and `cargo test --test config_writers` pass; `adapter_paths_match_legacy_layout` (apps/mod.rs:231) passes.
- [ ] `cargo clippy --all-targets -- -D warnings` and `cargo fmt --check` are clean.
- [ ] When ≤ 6: tick CCS-150's criterion, set it Done, update README.md and the CCS-005 epic row.

## Dependencies
CCS-232 (`p4-apps-budget-ratchet`).
