# Testing guide

How cc-switch is tested, where tests go, and how to mock the backend.

## The layers

| Layer                          | Where                                                                              | Runner                            | Command                                           |
| ------------------------------ | ---------------------------------------------------------------------------------- | --------------------------------- | ------------------------------------------------- |
| Pure frontend logic (no DOM)   | `src/**/*.test.ts`, `tests/{config,contracts,i18n,lib,scripts,utils}/**/*.test.ts` | vitest, `unit` project (node)     | `pnpm test:unit --project unit`                   |
| Components, hooks, integration | `tests/{app,components,hooks,integration}/**`, `tests/**/*.test.tsx`               | vitest, `dom` project (jsdom)     | `pnpm test:unit --project dom`                    |
| Renderer journeys (E2E)        | `e2e/*.e2e.mjs`                                                                    | node:test + Playwright (Chromium) | `pnpm test:e2e`                                   |
| Rust unit tests                | `#[cfg(test)]` modules next to the code                                            | cargo / nextest                   | `cargo test --lib <module>`                       |
| Rust integration tests         | `src-tauri/tests/*.rs` (shared `support.rs`)                                       | cargo / nextest                   | `cargo test --test <file>`                        |
| Proxy golden corpus            | `src-tauri/tests/fixtures/proxy/`                                                  | `proxy::testing`                  | `UPDATE_GOLDEN=1 cargo test --lib proxy::testing` |

The project split lives in `vitest.workspace.ts`. A `.ts` test that needs a DOM
can opt in with a `// @vitest-environment jsdom` first line.

## Where a new test goes

- A pure function: next to the source (`src/utils/foo.test.ts`) or under
  `tests/utils/` / `tests/lib/`. Never under `tests/components/`.
- A component: `tests/components/<Component>.test.tsx`.
- A hook: `tests/hooks/<hook>.test.tsx`.
- A user journey through the whole app: `e2e/journeys.e2e.mjs`.
- Rust: an inline `#[cfg(test)] mod tests` for unit logic; a file under
  `src-tauri/tests/` when the test goes through the public API, touches the
  filesystem or the command layer.

Every bug fix comes with a regression test (see the PR template); CI warns on
`fix` PRs that touch no test file.

## Mocking policy (frontend)

Mock **only at the IPC boundary**, plus toasts and timers.

- `@tauri-apps/api/core`'s `invoke` is routed to MSW (`tests/msw/tauriMocks.ts`).
  Default handlers live in `tests/msw/handlers.ts`, backed by the in-memory
  state in `tests/msw/state.ts`. **Every command a rendered component invokes
  needs a handler**; an unmocked invoke fails the test (`tests/setupTests.ts`).
- Per-test behaviour: `mockInvoke({ cmd: value | (args) => value })` from
  `tests/msw/invokeRecorder.ts` (it also records calls; assert the exact
  arguments with `recorder.callsTo("cmd")`), or `server.use(http.post(...))`.
- Error paths: reply with `tauriError(...)` from `tests/msw/tauriError.ts`.
  The rejection is exactly what the real backend sends: a plain string for
  `Result<_, String>` commands, or the `AppError` payload
  `{ code, message, params? }`. Contract samples come from
  `tests/contracts/app_error.json`, which `src-tauri/tests/contracts.rs`
  snapshots from the Rust serializer (`UPDATE_CONTRACTS=1` to regenerate).
- Do not `vi.mock("@/lib/api")`, sibling hooks or shadcn primitives in new
  tests; render the real thing against MSW with `createTestQueryClient()`
  (`tests/utils/testQueryClient.ts`).
- Test data: use the factories in `tests/factories/` (`makeProvider`,
  `makeSettings`, `makeMcpServer`, `makeSession`). Timestamps derive from
  `FIXED_NOW`, never `Date.now()`.
- Time: `vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout", "Date"] })`
  and `vi.advanceTimersByTime`, never real waits.
- Strings: prefer asserting user-visible English text. Load the real bundle
  with `i18n.addResourceBundle("en", "translation", en, true, true)` and
  `i18n.changeLanguage("en")` (see `tests/components/ProfileSwitcher.test.tsx`).
- `tests/contracts/commandRegistry.test.ts` checks that every `invoke("x")`,
  every generated binding and every MSW handler names a command registered in
  `src-tauri/src/lib.rs`.

## Rust test isolation

- Integration tests start with `let env = TestEnv::new();`
  (`src-tauri/tests/support.rs`): a fresh temp home, `CC_SWITCH_TEST_HOME`
  pointed at it, the settings cache reset. The override is honoured only in
  test builds (`test-hooks` feature, enabled through the self dev-dependency in
  `Cargo.toml`); release binaries ignore it.
- Unit tests that need a home directory use
  `crate::test_support::isolated_home()`: a per-thread temp home that ignores
  shared overrides, so the test needs no env vars and **no `#[serial]`**. Only
  tests that exercise env-var precedence themselves should use
  `test_support::EnvGuard` plus `#[serial]`.
- Home lookups always go through `crate::config::get_home_dir()`; clippy
  rejects direct `dirs::home_dir()` calls (`src-tauri/clippy.toml`).
- No `thread::sleep` in tests: backdate file mtimes (`File::set_modified`),
  backdate `Instant`s (see the circuit-breaker tests) or use tokio's paused
  clock.
- Property tests use `proptest`; case counts follow `PROPTEST_CASES` (the
  nightly workflow runs 2000).

## E2E journeys

`pnpm test:e2e` starts the Vite dev server with `e2e/ipc-mock.ts` injected
before the app entry. The mock installs `window.__TAURI_INTERNALS__` and
answers every invoke with the **same MSW handlers** the vitest suite uses.
Journeys can override a command with
`page.evaluate(() => { window.__E2E__.overrides.cmd = ... })` and read every
call from `window.__E2E__.calls`. On failure a screenshot and a Playwright
trace are written to `e2e/artifacts/` (uploaded by CI). Playwright is not a
package dependency: CI installs `playwright@1.63.0` globally; locally run with
`NODE_PATH="$(npm root -g)" pnpm test:e2e`.

Release-checklist mapping:

| Journey           | Release checklist item                                        |
| ----------------- | ------------------------------------------------------------- |
| J1 boot           | App starts, provider list renders, no unknown backend command |
| J2 switch         | Switching provider updates "In Use" and refreshes the tray    |
| J3 apps           | Every app tab loads its own providers                         |
| J4 switch failure | A backend error shows a localized toast and changes nothing   |
| J5 settings       | Settings page opens with all sections                         |
| J6 add provider   | Add-provider dialog opens with presets                        |

## CI

- `ci.yml`: typecheck, lint (incl. focused/disabled-test ban), prettier on
  `src` and `tests`, vitest with JUnit + coverage, the E2E journeys, clippy
  with `--all-targets --all-features`, and `cargo nextest` (retries; flaky
  tests are reported) with JUnit output.
- `nightly.yml`: high proptest case count, no retries, live S3 tests when the
  `S3_TEST_*` secrets are configured.
- `release.yml`: runs CI on the tagged commit, and the ARM64 release runners
  run the unit suites themselves before building.
