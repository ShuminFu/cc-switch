# CCS-183: [tray] Split the tray into model, menu and events driven by an event bus

**Status:** Done · **Kind:** refactor · **Labels:** `refactor`, `phase:P5`, `p3-low`, `area:shell`

> **Ticket key:** `p5-tray-split` · **Phase:** P5 · **Priority:** p3-low · **Effort:** M · **Depends on:** `p5-app-shell-bootstrap`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Evidence
- tray.rs:158-179 TRAY_SECTIONS, 477-483, 609, 675, 712 (block_on in menu build)

## Proposed change
Add tray/{model,menu,events}.rs. The model is derived from the registry and updated by events, with no block_on during menu builds.

## Acceptance criteria / verification
- [x] Unit test: the menu model for each state
- [x] `grep -n block_on src-tauri/src/tray` finds nothing
- [x] `cargo test --manifest-path src-tauri/Cargo.toml tray`

## Dependencies
p5-app-shell-bootstrap

## Implementation notes

**Summary.** `src-tauri/src/tray.rs` became the `tray/` module:

- `tray/model.rs` holds the pure menu model (`TrayMenuModel` / `MenuEntry`). `build_menu_model` builds it from the registry-derived `TRAY_SECTIONS`, the visible apps, providers, profiles, lightweight flag and a `TrayRuntime` snapshot (proxy running + taken-over apps). It has no Tauri, DB or async dependencies.
- `tray/menu.rs` collects the snapshot from DB/settings/usage cache, renders the model to a Tauri `Menu`, keeps the per-section submenu handles used for in-place usage relabelling, and provides `create_tray_menu` (sync; it reads the cached runtime snapshot) and `rebuild_tray_menu` (async; it probes `proxy_service.is_running()`, live backups and live-config takeover with `.await`, stores the snapshot, then rebuilds).
- `tray/events.rs` holds the tray event bus. `TrayEvent::{MenuChanged, UsageChanged}` goes into an unbounded channel with one consumer task, which coalesces events over a 50ms window (rebuild wins over relabel). `refresh_tray_menu` and `schedule_tray_refresh` now publish to it. The file also has the menu click handlers. Auto-failover click is now an async task that awaits the proxy calls and no longer waits on them synchronously.
- `tray/usage.rs` holds the usage-suffix formatting and the hover refresh (moved unchanged, along with their tests).
- Callers that rebuilt the menu inline (`commands/failover.rs`, `proxy/failover_switch.rs`) now publish a refresh. The `update_tray_menu` command awaits `rebuild_tray_menu`. Startup publishes a refresh after the proxy state is restored, so the runtime snapshot is probed. The grep guard in `apps/registry.rs` now points at `tray/mod.rs`.
- The public API used elsewhere is unchanged: `TRAY_ID`, `create_tray_menu`, `refresh_tray_menu`, `schedule_tray_refresh`, `handle_tray_menu_event`, `apply_tray_policy`, `refresh_all_usage_in_tray` and `detect_system_tray_language`.

**Files.** `src-tauri/src/tray/{mod,model,menu,events,usage}.rs` (replaces `src-tauri/src/tray.rs`), `src-tauri/src/lib.rs`, `src-tauri/src/app_shell/bootstrap.rs`, `src-tauri/src/commands/failover.rs`, `src-tauri/src/proxy/failover_switch.rs`, `src-tauri/src/apps/registry.rs`.

**Commits.** `e9dd3ee` (code), plus this ticket-notes commit.

**Tests.**

- `tray::model::tests` covers the menu model in each state: minimal menu, lightweight on, empty section placeholder, sorted providers with the current one checked and a usage suffix, missing current provider, official provider blocked while taken over, official provider selectable when the proxy is stopped or the app is not taken over, profile scope submenus with current and "none" items, profiles hidden, visibility filtering, and registry ordering.
- `tray::events::tests` covers event coalescing.
- `tray::tests::tray_sources_never_wait_synchronously_on_futures` is a source guard that fails on the old `tray.rs`.

**Verification.**

- `grep -n block_on src-tauri/src/tray` returns no matches.
- `cargo test --manifest-path src-tauri/Cargo.toml --lib tray`: 46 passed, 0 failed.
- `cargo test --manifest-path src-tauri/Cargo.toml --lib`: 2792 passed, 0 failed, 3 ignored.
- `cargo test --manifest-path src-tauri/Cargo.toml --no-fail-fast`: 3024 passed, 0 failed across all targets.
- `cargo fmt` is clean, and `cargo clippy --all-targets -- -D warnings` is clean.
- No frontend changes (the `update_tray_menu` command's JS signature is unchanged; the bindings test passes).
- Not verified here: the menu was not checked in a real desktop session (no display in this environment).
