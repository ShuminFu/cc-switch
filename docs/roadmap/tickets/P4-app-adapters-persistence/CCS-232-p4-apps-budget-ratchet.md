# CCS-232: [apps-tests] Make the match-app counter skip test-only modules and ratchet both app budgets to today's counts

**Status:** Done · **Kind:** chore · **Labels:** `chore`, `phase:P4`, `p2-medium`, `area:apps-tests`

> **Ticket key:** `p4-apps-budget-ratchet` · **Phase:** P4 · **Priority:** p2-medium · **Effort:** S
> Plan: [next-phase-plan.md](../../next-phase-plan.md)
> Found during the Partial-ticket triage at d962272 (CCS-005, CCS-149, CCS-150). Source tickets: [CCS-005](../P4-app-adapters-persistence/CCS-005-epic-p4.md), [CCS-149](../P4-app-adapters-persistence/CCS-149-p4-adapter-additive.md), [CCS-150](../P4-app-adapters-persistence/CCS-150-p4-adapter-conformance.md).

## Problem
Both P4 app-coupling budgets sit above what the code measures, so regressions can slip in:
- `MATCH_APP_SITE_BUDGET` is 40 (`src-tauri/src/apps/mod.rs:166`); the counter finds 30 sites. Two are false positives: `proxy/forwarder/wire_golden.rs` and `proxy/usage/cost_golden.rs` are declared `#[cfg(test)] mod ...;` in their parents but `match_app_sites()` only strips inline `#[cfg(test)]` blocks. The doc comment (160-165) still points at CCS-153/154/155, which are Done.
- `NEW_APP_BACKEND_FILE_BUDGET` is 10 (`src-tauri/tests/app_adapter_conformance.rs:870`); `new_app_backend_touch_points` reports 9 (settings.rs dropped out after CCS-155). The doc comment (863-869) still lists settings.rs.

## Proposed change
- In `apps::tests::match_app_sites`, first collect files declared under `#[cfg(test)] mod x;` in their parent (reuse `test_only_module_files` from tests/app_adapter_conformance.rs) and skip them. Expected count: 28.
- Set `MATCH_APP_SITE_BUDGET` to the new count (28) and `NEW_APP_BACKEND_FILE_BUDGET` to 9; update both doc comments.

## Acceptance criteria / verification
- [x] `cargo test --manifest-path src-tauri/Cargo.toml --lib apps::tests::match_app_sites_outside_apps_stay_within_budget` passes at 28.
- [x] `cargo test --manifest-path src-tauri/Cargo.toml --test app_adapter_conformance new_app_backend -- --nocapture` passes at 9.

## Dependencies
None.

## Implementation notes
- `apps::tests::match_app_sites` now reads all sources first, collects files declared via `#[cfg(test)] mod x;` with a local `test_only_module_files` (same logic as the helper in `tests/app_adapter_conformance.rs`; integration-test code cannot be imported into the lib), and skips them. `proxy/forwarder/wire_golden.rs` and `proxy/usage/cost_golden.rs` are no longer counted (30 -> 28 sites); the budget test also asserts these two files never appear.
- `MATCH_APP_SITE_BUDGET` 40 -> 28 and `NEW_APP_BACKEND_FILE_BUDGET` 10 -> 9; both doc comments updated (CCS-153/154/155 done, settings.rs no longer listed).
- Files: `src-tauri/src/apps/mod.rs`, `src-tauri/tests/app_adapter_conformance.rs`.
- Commit: b97d519 (code), plus this ticket-notes commit.

Verification:
- `cargo test --lib apps::tests::match_app_sites_outside_apps_stay_within_budget`: ok. With the budget temporarily set to 27 it failed with "28 match-app sites outside apps/** (budget 27)", so the count is exactly 28. Before the change the counter found 30, so a budget of 28 would have failed.
- `cargo test --test app_adapter_conformance new_app_backend -- --nocapture`: ok, prints "backend files a new app touches (9)"; whole conformance target 13 passed.
- `cargo fmt --check` clean; `cargo clippy --all-targets -- -D warnings` clean; `cargo test --lib`: 2858 passed, 0 failed, 3 ignored.
