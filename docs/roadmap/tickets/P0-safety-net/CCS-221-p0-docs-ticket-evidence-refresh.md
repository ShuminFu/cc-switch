# CCS-221: [docs] Tick already-verified acceptance boxes and refresh stale evidence in Partial tickets

**Status:** Open · **Kind:** chore · **Labels:** `chore`, `phase:P0`, `p3-low`, `area:docs`

> **Ticket key:** `p0-docs-ticket-evidence-refresh` · **Phase:** P0 · **Priority:** p3-low · **Effort:** S
> Plan: [next-phase-plan.md](../../next-phase-plan.md)
> Found during the Partial-ticket triage at d962272 (CCS-001, CCS-033, CCS-036, CCS-038, CCS-152, CCS-149). Source tickets: [CCS-001](../P0-safety-net/CCS-001-epic-p0.md), [CCS-033](../P0-safety-net/CCS-033-p0-ci-release-hygiene.md), [CCS-036](../P0-safety-net/CCS-036-p0-hotfix-terminal-tempfiles.md), [CCS-038](../P0-safety-net/CCS-038-p0-hotfix-session-sync-blocking.md), [CCS-152](../P4-app-adapters-persistence/CCS-152-p4-takeover-crash-e2e.md), [CCS-149](../P4-app-adapters-persistence/CCS-149-p4-adapter-additive.md).

## Problem
Several Partial tickets have boxes that are met in the current code but still `[ ]`, or evidence that points at moved code:
- CCS-033 AC1: `node scripts/check-versions.mjs v3.18.0` exits 1 (tag vs 3.17.0) and exits 0 with no argument, but the box is unticked. Its second Remaining bullet says Swatinem/rust-cache and attest-build-provenance are unpinned; CCS-028 pinned them (rust-cache d1527c8f… v2.9.3 at ci.yml 124/191/253/384 and release.yml 106; attest-build-provenance 977bb373… v3.0.0 at release.yml 726).
- CCS-036: the criterion command `cargo test ... commands::misc` and the notes point at `commands/misc.rs`; the code moved to `src-tauri/src/platform/terminal/{mod,tempfiles,posix,linux}.rs` (CCS-179/181).
- CCS-038: "cargo check + cargo test session_usage" and "debug log shows off-main-thread" are unticked though the notes record 41 passed, `test_run_sync_blocking_runs_off_caller_thread` and the `[SESSION-SYNC]` log. The 60 s loop now lives in `app_shell/bootstrap.rs:1068-1095`.
- CCS-152: `cargo test --test takeover_crash` gives 10 passed at d962272 (ticket says 5, box unticked); the "Known limitation" about hot-switch rollback is fixed by CCS-212 (`failed_hot_switch_keeps_the_takeover_snapshot`).
- CCS-149 follow-up: `src/config/hermesProviderSource.ts` doc comment still says `hermes_config.rs::get_providers` (now `apps/hermes/config.rs`).

## Proposed change
- Re-run each command below on the current tree; tick the box only when it passes, and record the command and result.
- CCS-033: tick AC1, rewrite the pinning bullet as done.
- CCS-036: update file paths and the criterion command to `platform::terminal`; tick the unit-test and test+clippy boxes.
- CCS-038: tick the two boxes; update the bootstrap.rs location.
- CCS-152: tick the takeover_crash box, update the count to 10, close the Known limitation with a pointer to CCS-212. Status stays Partial (3-OS item).
- Fix the hermesProviderSource.ts comment.

## Acceptance criteria / verification
- [ ] `node scripts/check-versions.mjs v3.18.0` exits 1; `node scripts/check-versions.mjs` exits 0; `npx vitest run tests/ci/releaseHygiene.test.ts tests/scripts/checkVersions.test.ts` passes.
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml --lib platform::terminal` and `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings` pass.
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml --lib session_usage` passes, including `test_run_sync_blocking_runs_off_caller_thread`.
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml --test takeover_crash` reports 10 passed.
- [ ] `pnpm typecheck` passes.

## Dependencies
None.
