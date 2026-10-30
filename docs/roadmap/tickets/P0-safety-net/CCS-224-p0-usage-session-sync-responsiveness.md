# CCS-224: [usage] Prove the UI stays responsive during a large session-log sync with a headless e2e spec

**Status:** Open · **Kind:** chore · **Labels:** `chore`, `phase:P0`, `p3-low`, `area:usage`

> **Ticket key:** `p0-usage-session-sync-responsiveness` · **Phase:** P0 · **Priority:** p3-low · **Effort:** M
> Plan: [next-phase-plan.md](../../next-phase-plan.md)
> Found during the Partial-ticket triage at d962272 (CCS-038, CCS-001). Source tickets: [CCS-038](../P0-safety-net/CCS-038-p0-hotfix-session-sync-blocking.md), [CCS-001](../P0-safety-net/CCS-001-epic-p0.md).

## Problem
CCS-038's code fix is in place (`sync_session_usage` is async and calls `sync_all_session_usage_blocking`, `commands/usage.rs:204-210`; `run_sync_blocking` at `services/session_usage.rs:834`), but the criterion "with many large .jsonl files, the window stays responsive during a manual sync" is marked manual. The container has the CCS-201 harness (`e2e/wdio.conf.mjs`, tauri-driver, WebKitWebDriver, xvfb-run), so it can be automated.

## Proposed change
- Add `e2e/specs/session-sync-responsiveness.e2e.mjs` (or an onPrepare fixture step): before launch, write ~200-500 Claude session `.jsonl` files (~200 MB) under `$CC_SWITCH_E2E_HOME/.claude/projects/<proj>/`, with assistant lines carrying `message.usage`, model and requestId (copy the shape from the session_usage.rs test fixtures).
- Open the Usage page and click sync (`src/components/usage/DataSourceBar.tsx`, handleSync 106, button 217; add a `data-testid` if needed).
- While syncing, time several `browser.execute` round-trips and run a rAF loop recording the largest frame gap; assert both stay under ~250 ms and the sync then reports `files_scanned > 0`.
- Negative control: make the command synchronous in a scratch build and confirm the spec fails.
- Optional: run with `RUST_LOG=cc_switch=debug` and assert `[SESSION-SYNC] ... running on thread` names a non-main thread.
- Cheaper extra/fallback: a Rust test in the session_usage tests module (~1238) that writes ~100 multi-MB files under `CC_SWITCH_TEST_HOME`, runs `sync_all_session_usage_blocking` on a current-thread runtime next to a 10 ms ticker, and asserts the max tick gap < ~100 ms and `files_scanned` matches.

## Acceptance criteria / verification
- [ ] `pnpm tauri build --debug --no-bundle && pnpm install --dir e2e --ignore-workspace && xvfb-run -a pnpm --dir e2e test` passes with the new spec.
- [ ] The negative control makes the spec fail.
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml --lib session_usage` passes (load test included if added).
- [ ] CCS-038's manual box ticked with a pointer to the spec.

## Dependencies
None.
