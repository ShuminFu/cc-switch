# CCS-183: [tray] Split the tray into model, menu and events driven by an event bus

**Status:** Open · **Kind:** refactor · **Labels:** `refactor`, `phase:P5`, `p3-low`, `area:shell`

> **Ticket key:** `p5-tray-split` · **Phase:** P5 · **Priority:** p3-low · **Effort:** M · **Depends on:** `p5-app-shell-bootstrap`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Evidence
- tray.rs:158-179 TRAY_SECTIONS, 477-483, 609, 675, 712 (block_on in menu build)

## Proposed change
Add tray/{model,menu,events}.rs. The model is derived from the registry and updated by events, with no block_on during menu builds.

## Acceptance criteria / verification
- [ ] Unit test: the menu model for each state
- [ ] `grep -n block_on src-tauri/src/tray` finds nothing
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml tray`

## Dependencies
p5-app-shell-bootstrap
