# CCS-241: [commands] Move the commands/settings.rs test module into its own file

**Status:** Open · **Kind:** refactor · **Labels:** `refactor`, `phase:P5`, `p3-low`, `area:commands`

> **Ticket key:** `p5-commands-settings-test-split` · **Phase:** P5 · **Priority:** p3-low · **Effort:** S
> Plan: [next-phase-plan.md](../../next-phase-plan.md)
> Found during the Partial-ticket triage at d962272 (CCS-006). Source tickets: [CCS-006](../P5-protocol-ir-commands/CCS-006-epic-p5.md).

## Problem
The P5 criterion "every `commands/*.rs` ≤ 600 lines" fails only on `src-tauri/src/commands/settings.rs` (1021 lines). `#[cfg(test)] mod tests {` starts at line 440, so production code is ~439 lines and tests ~582. The next largest file is import_export.rs (418).

## Proposed change
- Replace the inline module with `#[cfg(test)] #[path = "settings_tests.rs"] mod tests;` (or split into `commands/settings/mod.rs` + `tests.rs`) and move the body unchanged; `super::{merge_settings_for_save, validate_app_config_dir_override}` still resolves.
- Make sure `scripts/count-ipc-error-commands.mjs` and `check-command-bodies.mjs` skip the new tests file and still report 302/302.

## Acceptance criteria / verification
- [ ] `wc -l src-tauri/src/commands/*.rs` shows every file ≤ 600.
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml --lib commands::settings` runs the same number of tests.
- [ ] `node scripts/count-ipc-error-commands.mjs` and `node scripts/check-command-bodies.mjs` report 302/302.

## Dependencies
None.
