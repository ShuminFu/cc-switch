# CCS-224: [usage] Prove the UI stays responsive during a large session-log sync with a headless e2e spec

**Status:** Done · **Kind:** chore · **Labels:** `chore`, `phase:P0`, `p3-low`, `area:usage`

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
- [x] `pnpm tauri build --debug --no-bundle && pnpm install --dir e2e --ignore-workspace && xvfb-run -a pnpm --dir e2e test` passes with the new spec.
- [x] The negative control makes the spec fail.
- [x] `cargo test --manifest-path src-tauri/Cargo.toml --lib session_usage` passes (load test included if added).
- [x] CCS-038's manual box ticked with a pointer to the spec.

## Dependencies
None.

## Implementation notes

### Summary
- `e2e/specs/session-sync-responsiveness.e2e.mjs` (new). It writes 300 Claude
  session logs (~206 MB, 12,900 assistant messages) under
  `$CC_SWITCH_E2E_HOME/.claude/projects/-home-user-ccs224-session-sync/`.
  Lines have the same shape as the `session_usage.rs` fixtures: user and
  assistant lines alternate, and assistant lines carry `message.usage`, `model`
  and `requestId`. The spec writes the logs after launch, so the startup sync
  runs on an empty directory. It then opens Settings, goes to the Usage
  Statistics tab and runs `sync_session_usage` through
  `window.__TAURI_INTERNALS__.invoke`. While the sync runs, it times WebDriver
  `execute` round trips (including the call that starts the sync) and records
  the largest gap between `requestAnimationFrame` frames. Both must stay under
  250 ms. The result must show `filesScanned >= 300`, and at least 3 probes and
  3 frames must overlap the sync.
- Why the spec calls `invoke` instead of clicking a button: `DataSourceBar`
  (`src/components/usage/DataSourceBar.tsx`) is not mounted anywhere in the app,
  and has not been since the initial import. So the Usage page has no sync or
  re-scan button to click, and that includes the CCS-171 re-scan action. The
  spec makes the same IPC call that `handleSync` would make. Mounting the bar is
  a product decision and is out of scope here.
- `e2e/wdio.conf.mjs`: `specs` lists `smoke.e2e.mjs` first, because the smoke
  expects a first run on the fresh HOME and the new spec name would otherwise
  sort before it. WebdriverIO drops duplicate specs, so each spec still runs
  once. The new spec also confirms the first-run notice if it is shown.
- `src-tauri/src/services/session_usage.rs`: the projects-directory scan is
  split out of `sync_claude_session_logs_with_mode` into
  `sync_claude_projects_dir(db, projects_dir, mode)`, with no change in
  behaviour. A new load test, `test_large_session_sync_does_not_stall_async_runtime`,
  writes 48 x 1 MB logs to a temp dir. It syncs them through `run_sync_blocking`
  on a current-thread tokio runtime next to a 10 ms interval and asserts that
  the largest tick gap is under 100 ms, `files_scanned == 48`, `imported`
  equals the number of messages written, and there are no errors. The test
  takes the directory as a parameter instead of setting `CC_SWITCH_TEST_HOME`,
  because the process-wide env var would race with other tests.
- What each check covers: in the negative control, the rAF frame gap stayed
  low (97-99 ms), because the WebKit web process keeps painting while the UI
  process's main thread is blocked. The WebDriver round trip is the check that
  catches a main-thread sync. The frame-gap check still catches jank inside the
  page, such as refetch storms while rows are imported.
- Optional `RUST_LOG` thread-name assertion: not added. CCS-038's
  `test_run_sync_blocking_runs_off_caller_thread` already covers it, and the
  e2e negative control shows the same thing from the outside.
- CI: `ci.yml` (Linux e2e job) and `release.yml` (AppImage e2e) already run
  every `e2e/specs/**/*.e2e.mjs`, so the new spec runs in both with no workflow
  change.

### Files
- `e2e/specs/session-sync-responsiveness.e2e.mjs` (new)
- `e2e/wdio.conf.mjs`
- `src-tauri/src/services/session_usage.rs`
- `docs/roadmap/tickets/P0-safety-net/CCS-038-p0-hotfix-session-sync-blocking.md`
  (manual box ticked, pointing to the spec)

### Commits
- `b680a88` test(usage): headless e2e and load test for session-sync responsiveness (CCS-224)

### Verification (Linux, lane2/followups)
- `pnpm tauri build --debug --no-bundle && xvfb-run -a pnpm --dir e2e test`:
  2 spec files passed, 2 total. The smoke ran first (6/6) and the new spec
  passed 2/2. Over three runs: the sync took 12.2-13.9 s, 213-231 probes
  overlapped it, the largest round trip was 50-72 ms (baseline 16-48 ms), and
  the largest frame gap was 42-81 ms. The result was
  `{"filesScanned":300,"imported":12900,"skipped":0,"errors":[]}`.
- Negative control: in a scratch build, `sync_session_usage` was changed to a
  plain `pub fn` that calls `sync_all_session_usage(&state.db)`, so it runs on
  the main thread. The spec failed with `Expected: < 250, Received: 11375`: the
  call that started the sync blocked for the whole 11.4 s sync, and only 1
  probe overlapped it. The change was then reverted, the app rebuilt, and the
  spec passed again (the runs above).
- Rust load-test negative control: `run_sync_blocking` was temporarily changed
  to call `f()` inline. The test then failed with `async runtime stalled for
  2.558s during a 2.558s sync (0 ticks)`. It passed again after the change was
  reverted (4.4 s for the test binary).
- `cargo fmt --manifest-path src-tauri/Cargo.toml -- --check`: clean.
- `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings` and
  `--lib --tests -- -D warnings`: clean.
- `cargo test --manifest-path src-tauri/Cargo.toml --lib session_usage`: 57
  passed, 0 failed.
- `cargo test --manifest-path src-tauri/Cargo.toml --lib`: 2855 passed, 0
  failed, 5 ignored.
- `npx prettier --check e2e/specs/session-sync-responsiveness.e2e.mjs e2e/wdio.conf.mjs`:
  clean. Nothing under `src/` or `tests/` changed, so `pnpm typecheck`,
  `test:unit` and `format:check` are unaffected.
