# Testing & QA — findings (TEST-xx)

Part of the [next-phase proposals](../README.md). 33 findings. Priorities: P0 = 1, P1 = 8, P2 = 16, P3 = 8.

## Area summary

Frontend: 72 vitest files (68 under `tests/`: components 30, config 10, hooks 18, integration 2, lib 3, utils 5; plus 4 colocated `src/**/*.test.ts`) hold 462 tests and 12,966 LOC against 80,897 LOC of TS/TSX source (test:source ratio 0.16); whole component directories have zero tests (`src/components/proxy` 2,222 LOC, `universal` 1,173, `workspace` 812, `prompts` 608, `profiles` 484, `env` 275, `deeplink` 184, `hermes` 179), 10 of 26 hooks are never imported by a test, and the largest components never rendered by any test are `ProviderForm.tsx` (2,578 LOC; only its exported `normalizeCodexCatalogModelsForSave` helper is unit-tested), `CodexFormFields.tsx` (1,027), `ProxyPanel.tsx` (755) and `DeepLinkImportDialog.tsx` (730). Backend: `rg -c '#\[test\]|#\[tokio::test'` counts 2,060 inline test functions in 133 of 213 `.rs` files (~50.3k LOC of `mod tests` vs ~97.4k production LOC) plus 114 integration tests in 11 files under `src-tauri/tests/` (7,771 LOC), so the unit layer is dense (proxy 426 tests, proxy/providers 585, services 313, root modules 318) but 80 files have no tests at all, concentrated in the command layer (26 of 34 `commands/*.rs`, 285 `#[tauri::command]`s of which 192 take `State<>`/`AppHandle`, only 5 `*_test_hook` seams), the DAO layer (8 of 13 `database/dao/*.rs`), MCP sync (`mcp/codex.rs` 678 LOC, `services/mcp.rs` 546), the proxy server/failover plumbing (`proxy/server.rs` 405, `handler_config.rs` 228, `error.rs` 206, `failover_switch.rs` 135, `commands/proxy.rs` 455, `commands/failover.rs` 182) and `services/balance.rs` (454). Running the suite locally: `pnpm install --frozen-lockfile` succeeded (6.6 s), `pnpm typecheck` passed in 24 s, `pnpm test:unit` passed 72/72 files and 462/462 tests in 47.2 s (environment setup alone 39 s, slowest file `SessionManagerPage.test.tsx` 8.4 s) with 0 skipped tests, 0 act() warnings, but 18 `[MSW] Warning: intercepted a request without a matching request handler` (`get_installed_skills` x14, `list_profiles` x4), 4 `[App] Failed to sync window maximized state TypeError` from an unmocked `@tauri-apps/api/window`, 1 React unknown-prop warning, and 9 test files that fail `prettier --check` because `format:check` only scans `src/**`. The IPC mock layer (`tests/msw/`) is a faithful-in-spirit but tiny simulation: 58 handlers cover 58 of the 268 commands the frontend invokes (22%) and 285 commands registered in `lib.rs`, 5 handlers target commands that do not exist anywhere, there is no generated TS binding or contract check, and 15 of 72 test files bypass MSW by `vi.mock("@/lib/api")` wholesale. Backend tests are serialized through a process-global `test_mutex()` that is `.expect()`ed 109 times (a single panicking test poisons every later test in that binary), 123 `#[serial]` attributes and 130 `env::set_var` call sites compensate for global statics (`SETTINGS_STORE`, `APP_CONFIG_DIR_OVERRIDE`, `GLOBAL_CLIENT`), and there is no clock/uuid injection (64 `Utc::now()`, 14 `Uuid::new_v4()`), no proptest/insta/wiremock/cargo-fuzz, no `tauri::test` MockRuntime, no end-to-end proxy test (handlers.rs tests are pure-function tests; `proxy/server.rs` is never started in a test), no coverage provider installed, no E2E/UI harness, and CI runs `vitest` once on Ubuntu and `cargo test` on three OSes with no coverage gate, retries or JUnit output. Of the 50 commits available in this shallow clone, 26 are `fix`/`revert` and 11 of those sit in proxy format conversion / cache / usage accounting; 17 of 20 sampled fixes did add inline regression tests, so the missing piece is not discipline but infrastructure: golden corpora, property tests, an in-process proxy harness, command-layer tests and a contract between the TS API wrappers and the Rust command registry.

## Index

| ID | Title | Type | Priority | Effort | Confidence |
|---|---|---|---|---|---|
| [TEST-01](#test-01) | No in-process end-to-end test of the proxy request path | test | P0 | L | high |
| [TEST-02](#test-02) | Proxy format conversion has no golden-file corpus or whole-object assertions | test | P1 | L | high |
| [TEST-03](#test-03) | Tauri command layer (285 commands) is effectively untested | test | P1 | L | high |
| [TEST-04](#test-04) | MSW handler set drifts from the real command registry (22% coverage, 5 phantom handlers) | test | P1 | M | high |
| [TEST-05](#test-05) | No contract between TS API wrappers/types and Rust serde shapes | test | P1 | L | medium |
| [TEST-06](#test-06) | No coverage measurement or gate on either side | ci | P1 | M | high |
| [TEST-07](#test-07) | Zero tests for MCP sync services despite three recent MCP fixes | test | P1 | M | high |
| [TEST-08](#test-08) | Integration tests share one process-wide HOME, a poisonable global mutex and a file-backed DB | tech-debt | P1 | M | high |
| [TEST-33](#test-33) | Prioritized roadmap to a trustworthy pyramid | docs | P1 | XL | high |
| [TEST-09](#test-09) | Global mutable statics force 123 `#[serial]` tests and 130 env-var mutations | refactor | P2 | L | high |
| [TEST-10](#test-10) | Non-deterministic time/ids with no injection point (and a real `sleep(1s)` in a test) | tech-debt | P2 | M | high |
| [TEST-11](#test-11) | DAO layer mostly untested; two DAOs never touched by any test | test | P2 | M | high |
| [TEST-12](#test-12) | Frontend tests over-mock internal modules and assert weakly | test | P2 | M | high |
| [TEST-13](#test-13) | i18n is initialised with empty resources, so key regressions are invisible | test | P2 | S | high |
| [TEST-14](#test-14) | Tauri window/plugin APIs are not mocked; App tests log TypeErrors | test | P2 | S | high |
| [TEST-15](#test-15) | SSE/JSONL parsers lack fuzz/property coverage for arbitrary chunking and malformed input | test | P2 | M | medium |
| [TEST-16](#test-16) | Tray menu is never built in a test; only label formatting is covered | test | P2 | M | high |
| [TEST-17](#test-17) | No E2E/UI harness for the Vite renderer or the Tauri binary | test | P2 | L | high |
| [TEST-18](#test-18) | Frontend proxy, universal, prompts, profiles, workspace, env and deeplink components have zero tests | test | P2 | L | high |
| [TEST-19](#test-19) | Proxy failover and circuit-breaker state machines are under-tested and clock-coupled | test | P2 | M | medium |
| [TEST-20](#test-20) | Config parsers (TOML/YAML/JSON) have strong happy-path tests but almost no failure-path or property coverage | test | P2 | M | medium |
| [TEST-21](#test-21) | `format:check` and lint exclude the test tree; 9 test files already violate prettier | ci | P2 | S | high |
| [TEST-22](#test-22) | CI has no JUnit output, flaky-test detection, retries, or nightly tier | ci | P2 | M | high |
| [TEST-23](#test-23) | Three recent fixes landed without a regression test | test | P2 | S | high |
| [TEST-27](#test-27) | Usage accounting pipeline lacks a cross-version golden corpus despite 4 recent fixes | test | P2 | M | medium |
| [TEST-24](#test-24) | Frontend suite is slow per file and dominated by environment setup | performance | P3 | S | high |
| [TEST-25](#test-25) | No shared test-data factories; fixtures are copy-pasted | test | P3 | S | high |
| [TEST-26](#test-26) | MSW error responses do not resemble real Tauri invoke rejections | test | P3 | S | medium |
| [TEST-28](#test-28) | Network-facing services (balance, model fetch, speedtest, WebDAV/S3) have no mock-server tests | test | P3 | M | medium |
| [TEST-29](#test-29) | Inline test modules have grown into 1-2k-line blocks inside 3-7k-line source files | tech-debt | P3 | M | high |
| [TEST-30](#test-30) | Layering guard tests scan their own source text | tech-debt | P3 | S | high |
| [TEST-31](#test-31) | Test organisation is inconsistent (tests/ vs colocated, utils under components/) | docs | P3 | S | high |
| [TEST-32](#test-32) | Deep-link import is covered on the backend but the 730-line frontend dialog is untested | test | P3 | M | medium |

## Findings

### TEST-01: No in-process end-to-end test of the proxy request path

**Type:** test · **Priority:** P0 · **Effort:** L · **Confidence:** high

- **Evidence:** `src-tauri/src/proxy/server.rs` (405 LOC, 0 tests); `src-tauri/src/proxy/handlers.rs:2447-3089` — all 49 tests call pure helpers (`body_looks_like_sse`, `chat_sse_to_response_value`, ...), only 3 lines mention `Request::builder`; `rg 'TcpListener::bind\("127.0.0.1:0'` finds exactly 3 ad-hoc listeners, all in `src-tauri/src/services/coding_plan.rs:1977,2003,2107`; `src-tauri/Cargo.toml:112-114` dev-dependencies are only `serial_test` and `tempfile` (no wiremock/httpmock); the server is constructible without Tauri: `ProxyServer::new(config, db: Arc<Database>, app_handle: Option<AppHandle>)` at `src-tauri/src/proxy/server.rs:63-67`, `Database::memory()` at `src-tauri/src/database/mod.rs:181`, `ProxyConfig.listen_port` at `src-tauri/src/proxy/types.rs:9`; 11 of the 26 fix commits in `git log --oneline -50` are proxy/cache/usage fixes (613fef7, 6d316c0, 9ca1a41, 650905a, 27ce0a5, c6197ae, b9263a8, 0e563b5, f39d463, f991726, 13e7c1f).
- **Problem:** The highest-churn code (handlers -> forwarder -> transform -> streaming -> usage logger -> circuit breaker) is only tested piecewise. Nothing verifies that a real HTTP request entering the listener produces the right upstream request, streamed response, usage row and failover decision, so integration regressions (wrong header, double-encoding, lost tool-call id across the SSE re-chunking boundary) only surface in user reports.
- **Proposal:** Add `src-tauri/tests/proxy_e2e.rs` that (1) seeds `Database::memory()` with a provider whose `base_url` points at a `wiremock::MockServer`, (2) starts `ProxyServer::new(ProxyConfig{listen_port:0,..}, db, None)`, (3) sends Anthropic/OpenAI/Responses/Gemini requests with `reqwest` and asserts upstream request body + downstream response/SSE event sequence + `proxy_request_logs` row + circuit-breaker state. Add `wiremock`, `reqwest` (dev) to `[dev-dependencies]`. Cover: non-stream, stream, upstream 5xx -> failover to second provider, 413 mapping (`codex_proxy_413_points_to_upstream_not_local_proxy`), cache breakpoint injection on managed Claude takeover.
- **Acceptance:** `cargo test --test proxy_e2e` runs >= 12 scenarios on all 3 CI OSes in < 60 s; each scenario asserts the full upstream body via golden file (TEST-02); the three commits above could be reverted and at least one e2e scenario fails.

### TEST-02: Proxy format conversion has no golden-file corpus or whole-object assertions

**Type:** test · **Priority:** P1 · **Effort:** L · **Confidence:** high

- **Evidence:** inline `json!(` fixture calls inside test modules: `transform.rs` 122 (tests from line 725), `transform_responses.rs` 185 (from 859), `transform_codex_anthropic.rs` 154 (from 1450), `transform_codex_chat.rs` 124 (from 1838), `transform_gemini.rs` 84 (from 1169); whole-JSON equality assertions (`assert_eq!(x, json!(`) in those modules: 0 of 668 `assert_eq!`; sub-field index assertions (`assert_eq!(out["choices"][0]...`): 404; `find src-tauri -name fixtures -o -name golden` returns nothing (only `src-tauri/src/resources/*.json` templates); no `insta` in `Cargo.toml:112-114`.
- **Problem:** Each fix adds one more hand-written `json!` example that pins a few fields; nothing pins the complete converted payload, so a fix in one direction (e.g. 613fef7 reasoning forward, 9ca1a41 parameters type) can silently change other fields and the 3,500-line test modules are hard to review.
- **Proposal:** Create `src-tauri/tests/fixtures/conversion/<direction>/<case>.{input,expected}.json` and `.sse` corpora (anonymised captures of real Claude Code / Codex / Gemini CLI traffic, one per bug fixed so far), a table-driven test that runs every `input` through the adapter and compares to `expected` with `insta` (`cargo insta review` for intentional updates), and a `--features update-golden`-style workflow documented in CONTRIBUTING.md. Keep existing inline tests but stop adding new `json!` cases for full-body behaviour.
- **Acceptance:** >= 40 golden cases across anthropic<->openai, responses<->anthropic, codex-chat, gemini and streaming aggregation; every proxy fix PR adds a golden case; `cargo test conversion_golden` fails on any byte change of expected output.

### TEST-03: Tauri command layer (285 commands) is effectively untested

**Type:** test · **Priority:** P1 · **Effort:** L · **Confidence:** high

- **Evidence:** `rg -c '#\[tauri::command'` = 285; `rg -c 'State<' src-tauri/src/commands` = 192; 26 of 34 files in `src-tauri/src/commands/` have zero `#[test]` (e.g. `proxy.rs` 455 LOC, `workspace.rs` 362, `usage.rs` 344, `skill.rs` 336, `auth.rs` 305, `global_proxy.rs` 247, `copilot.rs` 221, `mcp.rs` 201, `profile.rs` 195, `failover.rs` 182, `import_export.rs` 176); the only seams are 5 `*_test_hook` functions (`commands/provider.rs:93-99,140-146`, `commands/proxy.rs:159-165,187-193,216-222,244-250`) and 6 `*_internal` fns; `rg 'tauri::test|mock_builder|MockRuntime'` = 0; `Cargo.toml:30` enables tauri features `["tray-icon","protocol-asset","image-png"]` but not `test`; `commands/misc.rs` holds 115 of the 162 command-layer tests, almost all for shell/path quoting.
- **Problem:** Argument parsing, error-string mapping (`.map_err(|e| e.to_string())` at `commands/provider.rs:27`), AppHandle event emission and tray refresh side effects are exercised only by the real app. A renamed argument or a changed `Result<_, String>` payload breaks the frontend without any failing test (see TEST-04/05).
- **Proposal:** (1) Add `tauri = { version = "2.8.2", features = ["test"] }` under `[dev-dependencies]` and a `src-tauri/tests/commands_ipc.rs` using `tauri::test::mock_builder()` + `tauri::test::get_ipc_response` to call commands through the real IPC deserialiser with `AppState` managed; (2) for the 192 `State<>` commands, mechanically extract `fn xxx_internal(state: &AppState, ...)` (pattern already used 6 times) and unit-test those directly; (3) retire the `test-hooks` feature (it is only a `doc(hidden)` gate at `commands/proxy.rs:159`) in favour of the MockRuntime harness.
- **Acceptance:** Every file in `src-tauri/src/commands/` has >= 1 test; `commands_ipc.rs` covers at least the 58 commands the frontend mocks today plus every command touched by a fix in the last 50 commits; `cargo llvm-cov` for `commands/` >= 60%.

### TEST-04: MSW handler set drifts from the real command registry (22% coverage, 5 phantom handlers)

**Type:** test · **Priority:** P1 · **Effort:** M · **Confidence:** high

- **Evidence:** command inventory (scratchpad `rust_cmds.txt`/`ts_cmds.txt`/`msw_cmds.txt`): 285 commands in `tauri::generate_handler![...]` (`src-tauri/src/lib.rs:1201`), 268 distinct `invoke("...")` names in `src/`, 58 handlers in `tests/msw/handlers.ts`, 216 invoked commands with no handler; handlers for commands that exist neither in Rust nor TS: `import_mcp_from_claude`/`import_mcp_from_codex` (`handlers.ts:176-177`), `select_config_directory` (`handlers.ts:260-270`), `reorder_failover_queue`/`set_failover_item_enabled` (`handlers.ts:355-356`); test run log: 18 `[MSW] Warning: intercepted a request without a matching request handler` (`POST http://tauri.local/get_installed_skills` x14, `list_profiles` x4) from `tests/components/SettingsDialog.test.tsx` and `tests/integration/SettingsDialog.test.tsx`; `tests/setupTests.ts:11` uses `onUnhandledRequest: "warn"`.
- **Problem:** Components silently render their error branches in tests (the unhandled invoke rejects), assertions pass against a broken page, and nobody notices phantom handlers because nothing cross-checks the three lists.
- **Proposal:** (1) Generate `tests/msw/commandRegistry.json` from `lib.rs` with a small script (`scripts/extract-commands.mjs`, run in CI) and add `tests/msw/registry.test.ts` asserting every TS `invoke` name and every MSW handler path is in the registry; (2) switch to `onUnhandledRequest: "error"` and add explicit handlers (or per-test `server.use`) for `get_installed_skills`, `list_profiles` and the other commands hit by component tests; (3) delete the 5 phantom handlers.
- **Acceptance:** registry test green in CI; test log contains 0 `[MSW] Warning` lines; handler count tracked in the registry test (>= 120 after Phase 1).

### TEST-05: No contract between TS API wrappers/types and Rust serde shapes

**Type:** test · **Priority:** P1 · **Effort:** L · **Confidence:** medium

- **Evidence:** `rg 'ts-rs|specta' src-tauri/Cargo.toml package.json` = none; `src-tauri/src/provider.rs:14-18` uses per-field `#[serde(rename = "settingsConfig")]` while `src-tauri/src/settings.rs:342` uses `rename_all = "camelCase"`; `src/lib/api/*.ts` (2,935 LOC, 26 files) hand-write 268 `invoke` calls with hand-written payload types; `src/lib/schemas/*.ts` (280 LOC, zod) are used only by 4 form components (`ProviderForm.tsx`, `BasicFormFields.tsx`, `ClaudeDesktopProviderForm.tsx`, `GrokBuildProviderForm.tsx`); `tests/msw/handlers.ts:253-256` returns a `get_config_dir` string while handler payloads are typed ad hoc (`withJson<{ app: AppId }>`).
- **Problem:** A renamed Rust field, a changed `Option` or a new enum variant is invisible to `tsc` and to vitest, because the mocks are typed from the TS side only. The `get_proxy_status` mock (`handlers.ts:315-333`) uses snake_case keys that are never validated against `src/types/proxy.ts`.
- **Proposal:** Either adopt `tauri-specta` to generate `src/lib/bindings.ts` from the command signatures (then `pnpm typecheck` becomes the contract test), or add a lighter contract suite: Rust tests serialise one sample of every response struct into `tests/contracts/<command>.json` (insta-managed), and a vitest file parses each file with the corresponding zod schema (extend `src/lib/schemas` to cover `Provider`, `Settings`, `ProxyStatus`, `McpServer`, `SessionMeta`, `UsageResult`). MSW handlers then return these fixtures instead of hand-typed literals.
- **Acceptance:** CI fails when a Rust struct field is renamed without updating the TS type; >= 30 response shapes covered; `tests/msw/handlers.ts` imports fixtures rather than literals for all non-trivial responses.

### TEST-06: No coverage measurement or gate on either side

**Type:** ci · **Priority:** P1 · **Effort:** M · **Confidence:** high

- **Evidence:** `vitest.config.ts:16-19` declares `coverage.reporter` but `ls node_modules/@vitest/` shows no `coverage-v8`/`coverage-istanbul` package and `package.json` devDependencies list none, so `vitest --coverage` cannot run; `.github/workflows/ci.yml:53-54,104-105` run `pnpm test:unit` and `cargo test` with no coverage step, thresholds, or artifact upload; no `cargo-llvm-cov`/`tarpaulin` anywhere.
- **Problem:** The team cannot see that `src/components/proxy` or `src-tauri/src/commands` are at ~0%, and nothing stops new untested modules from landing.
- **Proposal:** Add `@vitest/coverage-v8` and `coverage.include: ["src/**"]`, `exclude: ["src/components/ui/**","src/i18n/**","**/*.d.ts"]`, `thresholds` starting at measured baseline (ratchet upward via `thresholds.autoUpdate`); add a Linux-only CI job running `cargo llvm-cov --lcov` (`taiki-e/install-action@cargo-llvm-cov`) and upload both lcov files to Codecov (or `coverage-report` artifact + PR comment via `davelosert/vitest-coverage-report-action`). Gate on "no decrease > 0.5%" rather than absolute numbers initially.
- **Acceptance:** PR comment shows FE/BE coverage deltas; thresholds configured; a PR that adds a 200-line untested module fails the gate.

### TEST-07: Zero tests for MCP sync services despite three recent MCP fixes

**Type:** test · **Priority:** P1 · **Effort:** M · **Confidence:** high

- **Evidence:** `src-tauri/src/services/mcp.rs` (546 LOC, 0 tests; public fns `upsert_server`, `toggle_app`, `sync_all_enabled`, `sync_enabled_for_app`, `import_from_claude/codex/gemini/grokbuild/opencode` at lines 14-434), `src-tauri/src/mcp/codex.rs` (678 LOC, 0 tests; `import_from_codex:52`, `sync_enabled_to_codex:284`, `sync_single_server_to_codex:349`, `remove_server_from_codex:404`, all reading `crate::codex_config::get_codex_config_path()` at 342/360/408), `src-tauri/src/mcp/claude.rs` 149 LOC and `mcp/gemini.rs` 144 LOC untested; `src-tauri/src/database/dao/mcp.rs` 109 LOC untested; fixes 94fc1cc "surface per-app failures when importing MCP servers", 11c173c "stop cross-app failures from blocking MCP re-projection", 6d2ee24 "re-project Codex MCP after unified-session toggle" each added only 1-3 tests elsewhere; `src-tauri/tests/mcp_commands.rs` has 19 tests but goes through `McpService` only for Claude settings.json paths.
- **Problem:** TOML projection into `~/.codex/config.toml` (table vs inline-table forms, comment preservation, removal of stale entries) and the multi-app fan-out error isolation are the exact areas that regressed three times and remain unguarded.
- **Proposal:** Add `src-tauri/src/mcp/codex_tests.rs` (tempdir + `CC_SWITCH_TEST_HOME`) covering: import of `[mcp_servers.x]` table / inline table / legacy `mcpServers` JSON forms, idempotent `sync_enabled_to_codex` (write twice -> identical bytes), removal keeps unrelated keys and comments, malformed TOML returns `Err` not panic; and `services/mcp_tests.rs` with a fake writer per app proving that one failing app does not block the others and that the aggregated error names the failing app (regression for 94fc1cc/11c173c). Same pattern for `mcp/claude.rs`, `mcp/gemini.rs`, `mcp/opencode.rs`.
- **Acceptance:** >= 25 tests; reverting 94fc1cc or 11c173c makes at least one fail; mutation of the TOML table-detection branch in `mcp/codex.rs` is caught.

### TEST-08: Integration tests share one process-wide HOME, a poisonable global mutex and a file-backed DB

**Type:** tech-debt · **Priority:** P1 · **Effort:** M · **Confidence:** high

- **Evidence:** `src-tauri/tests/support.rs:7-24` sets `HOME`, `USERPROFILE`, `CC_SWITCH_TEST_HOME` process-wide to a single `cc-switch-test-home` dir; `support.rs:64-68` `test_mutex()`; 109 call sites of `test_mutex().lock().expect("acquire test mutex")` across `src-tauri/tests/*.rs` (e.g. `provider_service.rs:29`, `import_export_sync.rs:19`, `proxy_commands.rs:14`), 0 poison-tolerant (`into_inner`) uses; `support.rs:72-75` `create_test_state()` uses file-backed `Database::init()` while `Database::memory()` (`database/mod.rs:181`) is used 141 times in inline tests; `hermes_roundtrip.rs` has 2 tests but only 1 mutex lock; every test repeats the same 3-line preamble (`_guard`/`reset_test_fs()`/`ensure_test_home()`); `src-tauri/src/deeplink/tests.rs:15-30` implements its own competing HOME-swapping guard.
- **Problem:** One assertion failure poisons the mutex and every subsequent test in that binary fails with `PoisonError`, hiding the real failure (cascade); tests cannot run in parallel (`--test-threads` is forced to 1 by the mutex); on Windows the shared dir plus `remove_dir_all` is a known flake source (f6e37ed "repair platform-gated tests").
- **Proposal:** Replace the preamble with an RAII `TestEnv::new()` in a shared `src-tauri/tests/common/mod.rs` that (a) creates a per-test `tempfile::TempDir`, (b) acquires the lock with `lock().unwrap_or_else(PoisonError::into_inner)`, (c) uses `Database::memory()` unless `.with_disk_db()` is requested, (d) resets `update_settings(AppSettings::default())` on drop; reuse it from `deeplink/tests.rs`. Long term, make `get_app_config_dir()`/home resolution injectable (TEST-09) so the lock disappears.
- **Acceptance:** 0 `.expect("acquire test mutex")` left; a deliberately failing test no longer fails its siblings; `cargo test --test provider_service` wall time drops (measure before/after).

### TEST-33: Prioritized roadmap to a trustworthy pyramid

**Type:** docs · **Priority:** P1 · **Effort:** XL · **Confidence:** high

- **Evidence:** Aggregates TEST-01..32; current shape measured above: FE 462 unit/component tests + 0 e2e, BE 2,060 unit + 114 integration + 0 e2e; 80 backend files and 8 frontend directories with zero tests; no coverage data.
- **Problem:** Without sequencing, the team will keep adding inline `json!` tests per bug while the structural gaps (command layer, IPC contract, proxy e2e, determinism) stay open.
- **Proposal:** Phase 0 (week 1, S items): TEST-21 prettier/ESLint on tests, TEST-13 i18n parity, TEST-14 window/plugin mocks + console.error guard, TEST-23 three missing regression tests, TEST-04 phantom handlers + `onUnhandledRequest: "error"`. Phase 1 (weeks 2-4): TEST-06 coverage baseline + ratchet, TEST-08 `TestEnv` guard + memory DB, TEST-04 command registry contract, TEST-25 factories, TEST-01 proxy e2e harness with wiremock (first 5 scenarios), TEST-02 golden corpus seeded from the 11 proxy fixes. Phase 2 (weeks 5-8): TEST-03 MockRuntime command tests, TEST-07 MCP suites, TEST-11 DAO suites, TEST-18 proxy-panel/query-layer tests, TEST-10 clock injection, TEST-19 failover state machine, TEST-27 usage corpus, TEST-22 nextest/JUnit/nightly. Phase 3 (weeks 9-12): TEST-05 specta or contract fixtures, TEST-15 proptest + fuzz, TEST-16 tray snapshots, TEST-17 Playwright + tauri-driver smoke, TEST-20 config property tests, TEST-09 global-state refactor, TEST-28 network mocks, TEST-24/29/30/31 hygiene.
- **Acceptance:** Phase gates: P0 after Phase 1 = CI reports coverage, zero MSW warnings, proxy e2e green; after Phase 2 = every `commands/`, `dao/`, `mcp/` file has tests and `#[serial]` < 60; after Phase 3 = e2e journeys in CI, nightly fuzz/live tier, and FE/BE coverage >= 60%/70% with ratchet enforced.

### TEST-09: Global mutable statics force 123 `#[serial]` tests and 130 env-var mutations

**Type:** refactor · **Priority:** P2 · **Effort:** L · **Confidence:** high

- **Evidence:** `rg -c '#\[serial'` = 123 (`services/proxy.rs` 38, `services/provider/mod.rs` 26, `hermes_config.rs` 26, `proxy/provider_router.rs` 6, `app_config.rs` 6, ...); `rg -c 'env::set_var|set_var\('` = 130 across 19 files; statics: `SETTINGS_STORE` (`src-tauri/src/settings.rs:702`), `APP_CONFIG_DIR_OVERRIDE` (`app_store.rs:12`), `GLOBAL_CLIENT`/`CURRENT_PROXY_URL`/`CC_SWITCH_PROXY_PORT` (`proxy/http_client.rs:14-20`), `INIT_ERROR`/`MIGRATION_SUCCESS` (`init_status.rs:21-76`), `DB_CHANGE_TX` (`webdav_auto_sync.rs:18`, `s3_auto_sync.rs:18`), `LAST_TRAY_USAGE_REFRESH`/`TRAY_REBUILD_SCHEDULED` (`tray.rs:999-1006`); `services/proxy.rs` test module (from line 81) has 29 `set_var|update_settings|CC_SWITCH_TEST_HOME|Database::init()` uses; no `trait Clock`/`MockClock` anywhere.
- **Problem:** Tests that touch settings, paths or the HTTP client must run serially and mutate process state, which makes them slow, order-dependent and impossible to parallelise; a forgotten `#[serial]` becomes a flake that only reproduces under load.
- **Proposal:** Introduce an `AppContext { paths: Paths, settings: SettingsHandle, http: HttpClients, clock: Clock }` owned by `AppState` and threaded into services instead of the statics (start with `Paths` since `CC_SWITCH_TEST_HOME` already exists as the override seam); add a `clippy` lint-level `disallowed-methods` entry for `std::env::set_var` in non-test code and a test-side helper `with_env(...)` that restores values. Remove `#[serial]` as modules are migrated.
- **Acceptance:** `#[serial]` count < 30; `cargo test -- --test-threads=8` passes 5 consecutive runs; no `env::set_var` outside `tests/common`.

### TEST-10: Non-deterministic time/ids with no injection point (and a real `sleep(1s)` in a test)

**Type:** tech-debt · **Priority:** P2 · **Effort:** M · **Confidence:** high

- **Evidence:** backend non-test sources: `Utc::now()` 64, `Local::now()` 12, `SystemTime::now()` 19, `Instant::now()` 17, `Uuid::new_v4()` 14, clock abstraction 0; `src-tauri/tests/import_export_sync.rs:981` `std::thread::sleep(Duration::from_secs(1))` to obtain a distinct backup timestamp; `src-tauri/src/commands/webdav_sync.rs:203` and `commands/s3_sync.rs:195` tests assert ordering after `sleep(40ms)`; frontend: `Date.now()` 36 and `new Date(` 35 call sites in `src/`, only 2 test files use `vi.useFakeTimers` (`tests/hooks/useImportExport*.test.tsx`); `tests/msw/state.ts:27,35,47,55` seed `createdAt: Date.now()`.
- **Problem:** Backup naming, usage rollups, circuit-breaker timeouts, debounce windows (`usage_events.rs:59`, `tray.rs:1018`) and session sorting are all wall-clock dependent; tests either sleep (slow, flaky on loaded CI runners) or avoid the behaviour entirely.
- **Proposal:** Add `src-tauri/src/clock.rs` with `pub fn now() -> DateTime<Utc>` backed by a `thread_local!`/`OnceLock<RwLock<Option<FixedClock>>>` override for tests (or a `Clock` field in `AppContext` per TEST-09) and `ids::new_uuid()` with a deterministic test sequence; replace the 1 s sleep with a fixed-clock advance; on the frontend wrap `Date.now()` in `src/lib/clock.ts` and use `vi.useFakeTimers({ toFake: ["Date","setTimeout"] })` in tests of debounce/polling hooks (`useDebouncedValue`, `useProxyStatus`, `useAutoCompact`).
- **Acceptance:** 0 `thread::sleep` in `src-tauri/tests`; circuit-breaker and backup-retention tests run in < 10 ms each; `Date.now()` appears only in `src/lib/clock.ts`.

### TEST-11: DAO layer mostly untested; two DAOs never touched by any test

**Type:** test · **Priority:** P2 · **Effort:** M · **Confidence:** high

- **Evidence:** inline tests in `src-tauri/src/database/dao/`: `failover.rs` 0 (149 LOC), `mcp.rs` 0, `prompts.rs` 0, `settings.rs` 0 (327 LOC, 14 fns), `skills.rs` 0 (264 LOC), `stream_check.rs` 0, `universal_providers.rs` 0, `mod.rs` 0; `src-tauri/src/database/tests.rs` (17 tests, lines 155-895) references `stream_check` and `universal_providers` 0 times and `failover` 2 times; fix edea624 "preserve deleted default repositories" changed `dao/skills.rs` and added 2 tests to `database/tests.rs:155-186`; `database/migration.rs` 245 LOC untested beyond schema version tests.
- **Problem:** SQL in `settings.rs` (`get_bool_flag`, `set_config_snippet_cleared`, `get_proxy_takeover_enabled`), `failover.rs` (`add_to_failover_queue` ordering, `is_in_failover_queue`) and `skills.rs` (`init_default_skill_repos`) encodes product rules that are only checked indirectly through service tests, if at all.
- **Proposal:** Add `src-tauri/src/database/dao/tests/` (one file per DAO) on `Database::memory()` with CRUD round-trips, uniqueness/ordering invariants, and `proptest` round-trips for JSON-blob columns (`settings_config`, `meta`); add a migration test that opens every historical schema fixture (`tests/fixtures/db/v{N}.sql`) and asserts `migrate()` reaches `SCHEMA_VERSION` with row counts preserved (pattern exists once at `database/tests.rs:565`).
- **Acceptance:** every `dao/*.rs` has >= 3 tests; `cargo llvm-cov` for `database/dao` >= 70%; migration fixture test covers every version since v3.8.

### TEST-12: Frontend tests over-mock internal modules and assert weakly

**Type:** test · **Priority:** P2 · **Effort:** M · **Confidence:** high

- **Evidence:** 35 of 72 test files use `vi.mock`; 15 mock `@/lib/api` wholesale (`tests/components/SettingsDialog.test.tsx` 14 `vi.mock` calls, `UsageDashboard.test.tsx` 12, `McpFormModal.test.tsx` 11); `tests/hooks/useSettings.test.tsx:31-60` mocks three sibling hooks plus `@/lib/query` plus `@tanstack/react-query`'s `useQueryClient`; `tests/hooks/useProviderActions.test.tsx:48-83` re-implements `@/lib/query` and `@/lib/api` as `vi.fn()`s; 7 files replace shadcn primitives (`tests/components/WebdavSyncSection.test.tsx:30-72` mocks Button/Input/Switch/Select/Dialog); 18 mock `sonner`; 12 mock `react-i18next`; assertion mix: 94 bare `toHaveBeenCalled()` vs 116 `toHaveBeenCalledWith`, 140 `toBeInTheDocument`; `tests/components/McpFormModal.test.tsx:81-90` mock spreads unknown props to a `<textarea>` producing the `darkMode` React warning in the run log.
- **Problem:** Tests pass when the hook/component is refactored incorrectly as long as it still calls the mocked function; they also fail on harmless refactors (moving a call from `@/lib/api` to `@/lib/query`), which trains the team to update tests rather than trust them.
- **Proposal:** Adopt a rule "mock only at the IPC boundary (MSW) and for toasts/timers"; migrate the 15 `@/lib/api`-mocking files to MSW handlers + `createTestQueryClient()` (`tests/utils/testQueryClient.ts` already exists); replace bare `toHaveBeenCalled()` with argument assertions; add `eslint-plugin-testing-library` + `eslint-plugin-vitest` (`no-wait-for-multiple-assertions`, `prefer-user-event`, `no-conditional-expect`) since no ESLint exists (`rg eslint package.json` = none).
- **Acceptance:** <= 5 files mocking `@/lib/api`; ESLint runs in CI on `tests/**`; 0 React prop warnings in the test log.

### TEST-13: i18n is initialised with empty resources, so key regressions are invisible

**Type:** test · **Priority:** P2 · **Effort:** S · **Confidence:** high

- **Evidence:** `tests/setupTests.ts:13-22` calls `i18n.init({ lng: "zh", resources: { zh: { translation: {} }, en: { translation: {} } } })`; 92 assertions query raw keys, e.g. `screen.getByText("settings.tabAdvanced")` at `tests/integration/SettingsDialog.test.tsx:153-154,169-171`; 12 files additionally `vi.mock("react-i18next")` with `t: (key) => key`; commit 7e73a1f "add missing proxyReasonAnthropicMessages key across locales" is a shipped missing-key bug; locale files `src/i18n/locales/{en,ja,zh,zh-TW}.json` are not compared anywhere.
- **Problem:** A deleted, renamed or untranslated key renders as a raw key in production while every test still passes, and tests are coupled to key names instead of user-visible text.
- **Proposal:** Load the real `en.json` in `setupTests.ts` (`lng: "en"`), add `tests/i18n/localeParity.test.ts` asserting the four locale files have identical key sets and no empty strings, and `tests/i18n/usedKeys.test.ts` that extracts `t("...")` literals from `src/**` (simple regex + `i18next-parser` optional) and asserts each exists in `en.json`; migrate queries to English text/role names over time.
- **Acceptance:** parity test green; a PR that removes a key used by a component fails CI; `vi.mock("react-i18next")` count <= 3.

### TEST-14: Tauri window/plugin APIs are not mocked; App tests log TypeErrors

**Type:** test · **Priority:** P2 · **Effort:** S · **Confidence:** high

- **Evidence:** test log shows 4x `[App] Failed to sync window maximized state TypeError: Cannot read properties of undefined (reading 'metadata') at getCurrentWindow (@tauri-apps/api/window.js:85) at setupWindowStateSync (src/App.tsx:454)` in `tests/integration/App.test.tsx`; `tests/msw/tauriMocks.ts:7-65` mocks only `@tauri-apps/api/core`, `/event`, `/path`; `src/` imports `@tauri-apps/plugin-dialog`, `plugin-process` (x2), `plugin-updater` which no test mocks; `src/lib/platform.ts:4,14,23` detects platform via `navigator.userAgent` and only 1 test sets it.
- **Problem:** Any component touching window state, dialogs, process restart or updater is either untestable or exercises error branches silently; `App.test.tsx` passes while the real effect throws.
- **Proposal:** Extend `tests/msw/tauriMocks.ts` with `vi.mock("@tauri-apps/api/window")` (controllable `isMaximized`, `onResized` emitting through `emitTauriEvent`), `plugin-dialog` (`open`/`save` resolving from MSW state), `plugin-process` (`relaunch` spy), `plugin-updater` (`check` returning a fixture), plus a `setPlatform("macos"|"windows"|"linux")` helper that sets `navigator.userAgent`; assert no `console.error` in `afterEach` (fail on unexpected errors).
- **Acceptance:** test log has 0 `TypeError` lines; `App.test.tsx` asserts maximized-state sync; a `console.error` guard is enabled suite-wide with an explicit allow-list.

### TEST-15: SSE/JSONL parsers lack fuzz/property coverage for arbitrary chunking and malformed input

**Type:** test · **Priority:** P2 · **Effort:** M · **Confidence:** medium

- **Evidence:** `src-tauri/src/proxy/sse.rs:93-322` has 14 tests including split multibyte/CRLF cases (good model), but `proxy/providers/codex_responses_sse.rs` has 5 tests and 0 malformed-input keywords, `proxy/usage/parser.rs` 26 tests and 0 malformed-input keywords (`tests 517-1142`), `session_manager/providers/claude.rs` (10 tests) and `codex.rs` (20 tests) each have exactly 1 test mentioning malformed/partial input; `streaming_codex_chat.rs` fix 6d316c0 "preserve streamed tool call identity and order" (+164 LOC) is a chunk-ordering bug; no `proptest`/`arbitrary`/`cargo-fuzz` in `Cargo.toml` or repo.
- **Problem:** Streams from third-party gateways arrive with arbitrary chunk boundaries, BOMs, CRLF, truncated tails and non-JSON `data:` lines; hand-written cases cover each reported variant after the fact.
- **Proposal:** Add `proptest` dev-dependency and property tests: (a) for every golden SSE corpus file (TEST-02), any random partition into chunks fed through `SseAccumulator`/`streaming_*` produces output identical to single-chunk processing; (b) parsers never panic on arbitrary `Vec<u8>` (`prop_assert!(parse(bytes).is_ok() || is_err())`); (c) JSONL session parsers tolerate truncated last line and non-object lines. Add a `fuzz/` crate with `cargo-fuzz` targets for `sse`, `codex_responses_sse`, `usage::parser`, run weekly in CI (`cargo +nightly fuzz run ... -- -max_total_time=300`).
- **Acceptance:** >= 6 property tests; weekly fuzz job with corpus committed under `fuzz/corpus`; the 6d316c0 scenario expressed as a chunking property.

### TEST-16: Tray menu is never built in a test; only label formatting is covered

**Type:** test · **Priority:** P2 · **Effort:** M · **Confidence:** high

- **Evidence:** `src-tauri/src/tray.rs:637-640` `pub fn create_tray_menu(app: &AppHandle, app_state: &AppState) -> Result<Menu<Wry>, AppError>`; the 30 tests at `tray.rs:1133-1528` cover locale mapping and usage-summary strings, and `rg 'create_tray_menu|Menu::' ` inside the test module = 0; fix 997be22 "detect system locale for first-run language" touched `tray.rs` (+96 LOC) and `tray.rs:999-1018` holds a 50 ms debounce with global atomics; `services/proxy.rs`/`commands/misc.rs` call `update_tray_menu` on many state changes.
- **Problem:** Menu structure (sections per visible app, current-provider check marks, takeover/proxy toggles, language of every item) is the most visible UI on macOS/Windows and regresses silently; a snapshot per (language x app-visibility x proxy state) would catch it.
- **Proposal:** Split `create_tray_menu` into a pure `build_tray_model(settings, providers, proxy_status, texts) -> TrayModel` (data: nested items with ids/labels/checked/enabled) and a thin `render_menu(app, model)`; snapshot `TrayModel` with `insta` for the 4 languages x 3 visibility sets x proxy on/off; build the real `Menu` once under `tauri::test::mock_builder()` (TEST-03) to prove rendering does not panic.
- **Acceptance:** >= 24 insta snapshots in `src-tauri/src/snapshots/`; reverting 997be22 changes a snapshot; `create_tray_menu` body < 60 LOC.

### TEST-17: No E2E/UI harness for the Vite renderer or the Tauri binary

**Type:** test · **Priority:** P2 · **Effort:** L · **Confidence:** high

- **Evidence:** `rg 'playwright|webdriverio|tauri-driver|wdio' package.json` = none; no `e2e/` dir; `vite.config.ts:21-22` serves on port 3000 (`strictPort`), `src-tauri/tauri.conf.json` `devUrl: http://localhost:3000`; large UI never rendered in a test (`rg 'render\(.*<ProviderForm' tests` = none): `src/components/providers/forms/ProviderForm.tsx` 2,578 LOC (only `normalizeCodexCatalogModelsForSave` is unit-tested in `tests/components/ProviderForm.codexCatalog.test.ts`), `CodexFormFields.tsx` 1,027, `proxy/ProxyPanel.tsx` 755, `DeepLinkImportDialog.tsx` 730, `universal/UniversalProviderFormModal.tsx` 724; `tests/integration/App.test.tsx:24-70` replaces `ProviderList` and `AddProviderDialog` with stubs, so no test renders the real provider-switch flow end to end.
- **Problem:** The user journeys that define the product (add provider from preset -> switch -> see tray/live config updated; enable proxy takeover; import via deep link) are only verified manually before releases.
- **Proposal:** Phase A: Playwright against `pnpm dev:renderer` with an `initScript` that installs `window.__TAURI_INTERNALS__ = { invoke, metadata }` backed by the existing `tests/msw/state.ts` logic (reuse the same handler functions via a tiny adapter), covering 6-8 journeys with screenshots; run in the frontend CI job (`ubuntu`, ~2 min). Phase B: nightly `tauri-driver` + WebdriverIO smoke on Linux (`cargo tauri build --debug`, launch, assert window title, tray creation, one provider switch writing `~/.claude/settings.json` in a temp HOME).
- **Acceptance:** `pnpm test:e2e` green in CI with trace artifacts on failure; nightly Tauri smoke job; each journey maps to a release-checklist item in `docs/`.

### TEST-18: Frontend proxy, universal, prompts, profiles, workspace, env and deeplink components have zero tests

**Type:** test · **Priority:** P2 · **Effort:** L · **Confidence:** high

- **Evidence:** directory inventory (`src/components/<dir>`: files / LOC / test files referencing): proxy 8 / 2,222 / 0 (`ProxyPanel.tsx` 755, `AutoFailoverConfigPanel.tsx` 519, `CircuitBreakerConfigPanel.tsx` 356, `FailoverQueueManager.tsx` 311), universal 4 / 1,173 / 0, workspace 3 / 812 / 0, prompts 5 / 608 / 0, profiles 3 / 484 / 0, env 1 / 275 / 0, deeplink 3 / 184 / 0, hermes 1 / 179 / 0, agents 1 / 22 / 0; hooks never imported by a test: `useAutoCompact`, `useBackupManager`, `useDarkMode`, `useDebouncedValue`, `useLastValidValue`, `usePromptActions`, `useProxyConfig`, `useSessionSearch`, `useTauriEvent`, `useUsageCacheBridge`; query layer `src/lib/query/` 2,095 LOC (`mutations.ts` 394, `usage.ts` 392, `failover.ts` 290, `proxy.ts` 243) exercised unmocked by only 3 test files.
- **Problem:** The proxy control surface (toggle, takeover, failover queue, circuit breaker) is the feature with the most backend fixes and no frontend test; invalidation keys in `mutations.ts` are easy to break without noticing (stale lists after switch).
- **Proposal:** Add MSW-backed suites in this order: `tests/components/proxy/ProxyPanel.test.tsx` (status polling, start/stop, takeover per app, error toast), `FailoverQueueManager.test.tsx` (add/remove/reorder, enabled flag), `CircuitBreakerConfigPanel.test.tsx` (validation bounds), `tests/lib/query/mutations.test.tsx` (each mutation invalidates the documented keys — assert via `queryClient.getQueryState`), `DeepLinkImportDialog.test.tsx` (conflict preview/apply), `UniversalProviderFormModal.test.tsx`, `ProfileSwitcher.test.tsx`, plus unit tests for the 10 hooks. Add handlers for every command these use (TEST-04).
- **Acceptance:** every `src/components/*` directory has >= 1 test file; `src/lib/query` coverage >= 70%; 0 hooks without tests.

### TEST-19: Proxy failover and circuit-breaker state machines are under-tested and clock-coupled

**Type:** test · **Priority:** P2 · **Effort:** M · **Confidence:** medium

- **Evidence:** `src-tauri/src/proxy/failover_switch.rs` 135 LOC, 0 tests (`try_switch`); `proxy/circuit_breaker.rs` 4 tests; `proxy/provider_router.rs` 6 tests, all `#[serial]`; `commands/failover.rs` 182 LOC and `database/dao/failover.rs` 149 LOC 0 tests; `proxy/switch_lock.rs` 42 LOC 0 tests; `proxy/handler_config.rs` 228 LOC 0 tests; MSW mocks for the frontend side are static (`tests/msw/handlers.ts:347-383` always return `[]`/`true`).
- **Problem:** Open/half-open/closed transitions depend on `Instant::now()` and thresholds (`failureThreshold`, `timeoutSeconds`, `errorRateThreshold`, `minRequests`), and the switch lock prevents double failover; none of these timing paths are exercised deterministically.
- **Proposal:** With the injected clock (TEST-10), add table-driven tests for `CircuitBreaker` transitions, `FailoverSwitch::try_switch` (queue order, disabled entries, same-provider no-op, lock contention via two concurrent tasks), and `ProviderRouter` selection; add an e2e scenario (TEST-01) where upstream A returns 503 x `failureThreshold` and the next request lands on B with a `failover_count` increment in `get_status()`.
- **Acceptance:** >= 20 tests across the four files, none `#[serial]`, none sleeping.

### TEST-20: Config parsers (TOML/YAML/JSON) have strong happy-path tests but almost no failure-path or property coverage

**Type:** test · **Priority:** P2 · **Effort:** M · **Confidence:** medium

- **Evidence:** `src-tauri/src/codex_config.rs` 64 tests (`1920-3487`) with only 2 `is_err/expect_err` assertions; `hermes_config.rs` 57 tests (`1146-2410`) with 3; `opencode_config.rs` 267 LOC 0 tests (`read_opencode_config:88`, `set_provider:123`, `set_mcp_server:182`); `grok_config.rs` 516 LOC added in 1c0ee0c with 8 tests; `openclaw_config.rs` 5 tests; fix 88d5ffb moved TOML merging from `smol-toml` to `toml_edit` "to preserve comments" — property "parse(write(parse(x))) == parse(x)" and "comments survive set/remove" are asserted only on single examples (`codex_config.rs:2555 preserves_comments_and_whitespace`).
- **Problem:** Users hand-edit these files; invalid types, duplicate tables, unknown keys and encoding edge cases currently return whatever `toml_edit`/`serde_yaml` does, possibly panicking inside a Tauri command and surfacing as a frozen UI.
- **Proposal:** `proptest` strategies generating provider structs and arbitrary extra keys/comments; assert round-trip equality, idempotent writes, that unknown keys and comments are preserved byte-for-byte outside the edited table, and that every `read_*` on arbitrary bytes returns `Err` rather than panicking; add tests for `opencode_config.rs` mirroring `hermes_config.rs`'s `provider_crud_roundtrip`.
- **Acceptance:** >= 10 property tests across codex/hermes/opencode/openclaw/grok configs; `opencode_config.rs` >= 8 tests; `cargo test` with `PROPTEST_CASES=2000` passes in CI nightly.

### TEST-21: `format:check` and lint exclude the test tree; 9 test files already violate prettier

**Type:** ci · **Priority:** P2 · **Effort:** S · **Confidence:** high

- **Evidence:** `package.json:14` `"format:check": "prettier --check \"src/**/*.{js,jsx,ts,tsx,css,json}\""`; `pnpm exec prettier --check "tests/**/*.{ts,tsx}"` -> `Code style issues found in 9 files` (incl. `tests/hooks/useCommonConfigSave.test.tsx`, `tests/hooks/useSettings.test.tsx`, `tests/lib/keepLastGoodUsage.test.ts`, `tests/utils/providerConfigUtils.codex.test.ts`); no ESLint in `package.json`; `.github/workflows/ci.yml:50-51` runs only `pnpm format:check`.
- **Problem:** Test code quality is not held to the same bar as source; without ESLint there is no automated guard against `it.only`, floating promises in tests, or testing-library anti-patterns.
- **Proposal:** Change `format`/`format:check` globs to `"{src,tests}/**/*.{js,jsx,ts,tsx,css,json}"`, run `prettier --write tests` once, add ESLint (flat config) with `@typescript-eslint`, `eslint-plugin-testing-library`, `eslint-plugin-vitest` (`vitest/no-focused-tests`, `vitest/no-disabled-tests` as warn), `eslint-plugin-react-hooks`; add `pnpm lint` to CI.
- **Acceptance:** `pnpm format:check` and `pnpm lint` cover `tests/`; CI fails on `it.only`.

### TEST-22: CI has no JUnit output, flaky-test detection, retries, or nightly tier

**Type:** ci · **Priority:** P2 · **Effort:** M · **Confidence:** high

- **Evidence:** `.github/workflows/ci.yml:14-54` single `ubuntu-latest` frontend job (`pnpm test:unit` with default reporter, no artifacts); `ci.yml:56-105` backend matrix runs `cargo test` (no `--locked`, no `nextest`, no per-test timeout, no `--features`), `concurrency.cancel-in-progress: true` (`ci.yml:9-11`); 2 `#[ignore]` live S3 tests (`src-tauri/src/services/s3.rs:882-893`) are never run anywhere; `release.yml` contains no test step; `pnpm/action-setup@v6` pins pnpm 10.12.3 (`ci.yml:29`) while `package.json` has no `packageManager` field, so local (10.28.0 in this run) and CI pnpm versions can diverge.
- **Problem:** When a test flakes there is no history to find it, no retry to classify it, and no structured output for PR annotations; slow/network tests have no home so they are ignored forever.
- **Proposal:** (1) `vitest run --reporter=default --reporter=junit --outputFile=reports/vitest.xml` + `actions/upload-artifact` + `dorny/test-reporter`; (2) `cargo nextest run --profile ci` with `retries = 2` and `slow-timeout = { period = "60s", terminate-after = 2 }` in `.config/nextest.toml`, which also surfaces flaky tests explicitly; (3) a `nightly.yml` workflow running `cargo test -- --ignored` with `S3_TEST_*` secrets, proptest with high case counts, fuzz (TEST-15), and the Tauri smoke (TEST-17); (4) run vitest on `windows-latest` too (path-sensitive utils like `providerConfigUtils`, `tomlUtils`).
- **Acceptance:** PR checks show per-test results; nextest flaky report visible; nightly workflow green for 2 weeks.

### TEST-23: Three recent fixes landed without a regression test

**Type:** test · **Priority:** P2 · **Effort:** S · **Confidence:** high

- **Evidence:** `git show --format= <sha> | grep -cE '^\+\s*(#\[test\]|#\[tokio::test|fn test_|it\(|test\()'` = 0 for 9ca1a41 (`normalize_function_parameters` at `src-tauri/src/proxy/providers/transform_codex_chat.rs:1146`), af58740 (`proxy/providers/auth.rs:124`, `claude.rs:24-31,850`), f2c6d48 (official-provider skip at `services/stream_check.rs:169-171`, `commands/stream_check.rs:78`); the other 17 sampled fixes added 1-36 tests each, so the norm exists but is not enforced; no PR template in `.github/`.
- **Problem:** These three branches (strict OpenAI-compatible providers rejecting non-object `parameters`, Codex OAuth client identity header, reachability probes against official endpoints) can regress unnoticed.
- **Proposal:** Add the three tests now (`normalize_function_parameters` table test; `ClaudeAdapter` auth header snapshot for OAuth vs API-key; `stream_check` returns `Err` and skips probes for `category == "official"`), and add `.github/pull_request_template.md` with a "Regression test added / N/A because" checkbox; optionally a lightweight `danger`/script step that warns when a `fix:` PR touches no test file.
- **Acceptance:** 3 tests merged; template present; the warning step runs on PRs.

### TEST-27: Usage accounting pipeline lacks a cross-version golden corpus despite 4 recent fixes

**Type:** test · **Priority:** P2 · **Effort:** M · **Confidence:** medium

- **Evidence:** fixes f39d463 (Codex sub-agent usage not counted), f991726 (cache-write tokens across schema versions), 13e7c1f (Anthropic cache-write TTLs), 98ccde0 (dashboard refresh interval) and revert 6eb217b (TTL-bucketed accounting) all touch `proxy/usage/{parser,calculator,logger}.rs`, `services/usage_stats.rs`, `services/session_usage_*.rs`, `database/dao/usage_rollup.rs`; tests exist (`usage/parser.rs` 26, `usage_stats.rs` 29, `usage_rollup.rs` 10, `session_usage_codex.rs` 20) but every fixture is an inline `json!`; `database/tests.rs:381` tests the v10->v11 rollup rebuild once; `proxy/usage/logger.rs` has 3 tests.
- **Problem:** Pricing/cost numbers shown to users depend on provider-specific usage payloads (Claude, OpenRouter, Codex Responses, Gemini thoughts) and on rollup migrations; a change in one parser silently shifts totals and there is no corpus of real payloads to replay.
- **Proposal:** Create `src-tauri/tests/fixtures/usage/<provider>/<case>.json` (+ expected `UsageRecord`) replayed through parser -> calculator -> logger into `Database::memory()`, then assert rollup totals and the dashboard query output; include one fixture per fix above and one per pricing seed (`gpt-5.6`, `Hunyuan Hy3`).
- **Acceptance:** >= 25 usage fixtures; an end-to-end test from raw SSE to `usage_rollup` totals; reverting f991726 fails a replay.

### TEST-24: Frontend suite is slow per file and dominated by environment setup

**Type:** performance · **Priority:** P3 · **Effort:** S · **Confidence:** high

- **Evidence:** run summary `Duration 47.20s (transform 6.22s, setup 27.19s, collect 18.25s, tests 33.03s, environment 39.18s)`; slowest files `SessionManagerPage.test.tsx` 8,396 ms (14 tests, 31 `waitFor`), `GrokBuildProviderForm.test.tsx` 4,405 ms (5 tests), `App.test.tsx` 4,392 ms (4 tests), `SettingsDialog` integration 2,203 ms; `vitest.config.ts:13-20` uses `environment: "jsdom"` globally with no `pool`, `isolate`, `projects` or per-file `// @vitest-environment node` for pure-util tests (`tests/config/*`, `tests/utils/*`, `tests/lib/*` = 18 files that need no DOM); `tests/integration/SettingsDialog.test.tsx:179-181` needs `timeout: 4000`.
- **Problem:** 39 s of the 47 s is spinning up jsdom; as the suite grows toward the sizes proposed above this becomes the bottleneck and encourages skipping tests locally.
- **Proposal:** Use vitest `workspace`/`projects`: `unit` (environment node, `tests/{config,utils,lib}/**`, `src/**/*.test.ts`), `component` (jsdom or happy-dom, `tests/{components,hooks}/**`), `integration` (jsdom, `tests/integration/**`); set `pool: "threads"` with `isolate: false` for the node project; replace `waitFor` polling on async queries with `findBy*`; use fake timers in `SessionManagerPage` tests where debounce is involved.
- **Acceptance:** full `pnpm test:unit` < 25 s on the CI runner; pure-util project < 5 s; no `waitFor` with custom timeout.

### TEST-25: No shared test-data factories; fixtures are copy-pasted

**Type:** test · **Priority:** P3 · **Effort:** S · **Confidence:** high

- **Evidence:** `settingsConfig: {}` literal provider fixtures appear 18 times across `tests/`; separate `createProvider` helpers in `tests/hooks/useProviderActions.test.tsx:97-106`, default providers in `tests/msw/state.ts:19-76`, inline providers in `tests/integration/App.test.tsx`; backend tests rebuild `Provider::with_id(..., json!({...}), None)` by hand in `src-tauri/tests/provider_service.rs:39-50`, `import_export_sync.rs:24-39`, `mcp_commands.rs:27-36`; no `tests/factories/` or Rust `test_support` module.
- **Problem:** Adding a required field to `Provider`/`Settings` means editing dozens of literals; fixtures drift from realistic shapes (e.g. no `meta`, no `createdAt`), so tests do not reflect production data.
- **Proposal:** Add `tests/factories/{provider,settings,mcp,session,usage}.ts` (`makeProvider(overrides)` with realistic defaults per app) and reuse them in `tests/msw/state.ts`; add `src-tauri/src/test_support.rs` (`#[cfg(any(test, feature = "test-support"))]`) with `ProviderBuilder`, `SettingsBuilder`, `seed_db(&Database)`; derive factories from the contract fixtures (TEST-05) where possible.
- **Acceptance:** `settingsConfig: {}` literal count <= 3; all new tests import factories; one place to update when a type changes.

### TEST-26: MSW error responses do not resemble real Tauri invoke rejections

**Type:** test · **Priority:** P3 · **Effort:** S · **Confidence:** medium

- **Evidence:** `tests/msw/tauriMocks.ts:17-19` turns any non-2xx into `new Error(text || ...)`; `tests/msw/handlers.ts:88-90` returns `HttpResponse.json(false, { status: 404 })` so the rejection message is the string `"false"`; real commands reject with the Rust `String` (`commands/provider.rs:26-27 Result<_, String>`) or serialised `AppError::Localized { key, .. }` (`src-tauri/tests/proxy_commands.rs:36-42`), which the UI maps to i18n keys.
- **Problem:** Error-path tests cannot assert the user-visible message, so toast/error-boundary behaviour for localized backend errors is unverified.
- **Proposal:** Add `tauriError(message | { key, params })` helper in `tests/msw/` that returns the exact JSON the Rust `AppError` serialiser produces (add a Rust test that snapshots that serialisation into `tests/contracts/app_error.json`), use it in handlers, and add tests asserting `toast.error` receives the translated key.
- **Acceptance:** handlers never return bare `false` with an HTTP status; >= 5 error-path tests assert localized keys.

### TEST-28: Network-facing services (balance, model fetch, speedtest, WebDAV/S3) have no mock-server tests

**Type:** test · **Priority:** P3 · **Effort:** M · **Confidence:** medium

- **Evidence:** `src-tauri/src/services/balance.rs` 454 LOC, 0 tests (`get_balance:426`); `services/model_fetch.rs` 22 tests (parsing only), `services/speedtest.rs` 3, `services/stream_check.rs` 11, `services/webdav.rs` 7, `services/s3.rs` 28 of which the only live round-trips are `#[ignore]` (`s3.rs:882-893`, require `S3_TEST_ENDPOINT`); no HTTP mock crate in dev-dependencies.
- **Problem:** Provider balance endpoints (Zhipu, SubRouter, etc.) change response shapes frequently; WebDAV/S3 sync correctness (ETag handling, conflict resolution, archive format) is only checked against live services nobody runs.
- **Proposal:** With `wiremock` (TEST-01), add per-provider balance fixture tests (`tests/fixtures/balance/<provider>.json`), WebDAV PROPFIND/PUT/GET flows and S3 signed-request assertions (verify `Authorization` header shape and `x-amz-content-sha256`) against the mock server; keep live tests in the nightly job (TEST-22).
- **Acceptance:** `balance.rs` >= 10 tests; WebDAV and S3 sync each have an upload->download->merge round-trip against wiremock; nightly live job configured.

### TEST-29: Inline test modules have grown into 1-2k-line blocks inside 3-7k-line source files

**Type:** tech-debt · **Priority:** P3 · **Effort:** M · **Confidence:** high

- **Evidence:** `src-tauri/src/proxy/forwarder.rs` 4,769 LOC with tests from line 3,178; `services/proxy.rs` 7,215 LOC with the first `#[cfg(test)]` at line 81 and 3 test blocks; `proxy/providers/transform_codex_chat.rs` 3,529 LOC, tests from 1,838; `codex_config.rs` 3,497 LOC, tests from 1,914; approx 50.3k LOC of inline tests across 131 files; `commands/misc.rs` holds 115 tests after line 3,543.
- **Problem:** Every test edit recompiles the whole production module, reviewers cannot see test diffs in isolation, and the `#[serial]` sprawl is harder to audit.
- **Proposal:** Move each large block to a sibling file (`#[cfg(test)] mod tests;` -> `forwarder/tests.rs` or `forwarder_tests.rs`) and split by feature (`tests/streaming.rs`, `tests/failover.rs`); keep small modules inline. Combine with `cargo nextest` for incremental runs.
- **Acceptance:** no source file > 2,500 LOC containing a test module; `cargo test -p cc-switch-lib --lib forwarder` incremental rebuild time measured and reduced.

### TEST-30: Layering guard tests scan their own source text

**Type:** tech-debt · **Priority:** P3 · **Effort:** S · **Confidence:** high

- **Evidence:** `src-tauri/src/services/webdav_auto_sync.rs:265-272` `service_layer_does_not_depend_on_commands_layer` uses `include_str!("webdav_auto_sync.rs")` and `.contains("crate::commands::")`; identical pattern at `services/s3_auto_sync.rs:263`; no equivalent guard for the other 29 service files or for `proxy/` -> `commands/`.
- **Problem:** The rule is enforced for 2 of 31 services and is trivially bypassed by `use crate::commands;` + `commands::x` or by `super::` paths.
- **Proposal:** Replace with one architecture test that walks `src-tauri/src/services/**/*.rs` and `proxy/**/*.rs` parsing `use` statements with `syn` (dev-dep) and asserting no path starts with `crate::commands`; or enforce via `cargo-modules`/`cargo deny` in CI.
- **Acceptance:** single test covers all service/proxy files; the two `include_str!` tests removed.

### TEST-31: Test organisation is inconsistent (tests/ vs colocated, utils under components/)

**Type:** docs · **Priority:** P3 · **Effort:** S · **Confidence:** high

- **Evidence:** 4 colocated tests in `src/` (`src/lib/version.test.ts`, `src/utils/{grokBuildConfig,providerConfigUtils,providerIcon}.test.ts`) vs 68 under `tests/`; pure-function tests living in `tests/components/` (`openclaw.utils.test.ts`, `sessionUtils.test.ts`, `usageFormat.test.ts`, `ProviderCardLayout.test.ts`, `SelectContentScroll.test.ts`); hook tests for `src/components/providers/forms/hooks/*` placed in `tests/hooks/`; `CONTRIBUTING.md:44-68` documents only the commands, not where tests go or how to mock IPC; `vitest.config.ts` has no `include`.
- **Problem:** Contributors (and the Claude review bot in `.github/workflows/claude.yml`) have no rule to point to, so new tests land in arbitrary places and mocking style varies per file.
- **Proposal:** Write `tests/README.md` (layout, MSW-first mocking policy, factories, i18n policy, how to add a handler, how to run one project) and link it from CONTRIBUTING.md; pick one convention (colocate pure unit tests next to source, keep component/integration under `tests/`) and move the 5 misplaced files; set explicit `include` globs per vitest project (TEST-24).
- **Acceptance:** README merged; `tests/components/` contains only component tests; CONTRIBUTING links the policy.

### TEST-32: Deep-link import is covered on the backend but the 730-line frontend dialog is untested

**Type:** test · **Priority:** P3 · **Effort:** M · **Confidence:** medium

- **Evidence:** backend: `src-tauri/src/deeplink/tests.rs` 31 tests + `src-tauri/tests/deeplink_import.rs` 2 tests; frontend: `src/components/DeepLinkImportDialog.tsx` 730 LOC and `src/components/deeplink/*` (184 LOC) referenced by 0 tests; `src/lib/api/deeplink.ts` 104 LOC with no MSW handler for `import_from_deeplink`/`parse_deeplink` (`rg deeplink tests/msw/handlers.ts` = none); `grokbuild` support (1c0ee0c) added deeplink provider parsing with 95 backend test lines and 0 frontend tests.
- **Problem:** Deep links are an external attack/UX surface (arbitrary URLs produce provider configs); the preview/confirm UI that protects users is unverified.
- **Proposal:** Add handlers + `tests/components/DeepLinkImportDialog.test.tsx` covering: preview renders parsed provider/MCP/prompt/skill payloads, conflict warnings, cancel does not invoke import, import success triggers query invalidation, malformed link shows error; share the backend's deeplink fixtures (TEST-05 contract fixtures).
- **Acceptance:** >= 8 tests; `import_from_deeplink` handler present; shared fixture reuse demonstrated.
