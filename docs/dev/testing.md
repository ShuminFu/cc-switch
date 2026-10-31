# Testing

How the test suites are organised and how to reproduce every CI check locally.

## Run what CI runs

```bash
pnpm install
pnpm check          # typecheck, ESLint, Prettier, i18n integrity, repo checks, unit tests
pnpm check:rust     # creates dist/ if missing, cargo fmt --check, clippy (all targets and features, -D warnings), cargo test --locked
```

`pnpm check` and `pnpm check:rust` mirror the `frontend` and `backend` jobs in
`.github/workflows/ci.yml`. CI additionally:

- runs the backend job on Linux, Windows and macOS, and an ARM64 job (`cargo test` on
  `ubuntu-22.04-arm`, `cargo check --all-targets` on `windows-11-arm`);
- builds the renderer and checks its size budget (`pnpm build:renderer && node scripts/bundle-report.mjs`);
- runs `cargo deny` (licenses, bans, sources, advisories) and `pnpm audit --prod`;
- verifies `src/lib/bindings.ts` is up to date (`cargo test --lib commands::bindings`).

## Frontend (vitest)

- Tests live in `tests/` (mirroring `src/`) and run in jsdom: `pnpm test:unit`,
  `pnpm test:unit:watch`, `pnpm test:unit -- <pattern>`.
- Tauri IPC is mocked with MSW (`tests/msw/`). An `invoke` without a handler fails the test, so
  add a default handler in `tests/msw/handlers.ts` for every new command.
- Coverage: `pnpm test:coverage` writes `coverage/lcov.info` (V8). CI uploads it to Codecov
  (non-blocking) and publishes a JUnit summary.
- `tests/contracts/` checks the IPC contract (every generated binding is registered in `lib.rs`).
- `tests/i18n/` checks locale parity and key usage.
- `tests/scripts/` tests the repository scripts in `scripts/`.

## Backend (cargo test)

- Unit tests sit next to the code in `#[cfg(test)]` modules; integration tests are in
  `src-tauri/tests/`, each using `support::TestEnv`, which points `HOME`/`USERPROFILE` and
  `CC_SWITCH_TEST_HOME` at a temporary directory so nothing touches your real config.
- Tests that change process-global state (environment variables, the settings store) are marked
  `#[serial]` (serial_test). Keep new ones isolated the same way, and use `tempfile` rather than
  fixed paths under the system temp directory.
- Keychain access is replaced by an in-memory backend whenever `CC_SWITCH_TEST_HOME` is set.
- Targeted runs: `cargo test --manifest-path src-tauri/Cargo.toml --lib <module>` or
  `--test <file>`.
- The `test-hooks` feature only un-hides some commands from rustdoc; clippy lints it in CI with
  `--all-features`.
- The build needs `dist/` to exist (`tauri::generate_context!` reads `frontendDist`); `pnpm check:rust`
  creates it, otherwise run `mkdir -p dist`.

## Writing good tests

- Every bug fix gets a regression test that fails without the fix.
- Prefer testing a service or adapter function over a full command when the logic lives there.
- For live-file changes, assert the file **byte for byte** after a round trip (keys, comments and
  ordering the app does not own must survive).
- Do not skip or delete an existing test to make a change pass; if it encodes the old behaviour,
  update it and explain why in the PR.

## Not covered yet

- No end-to-end UI tests (tauri-driver/Playwright) and no scripted multi-step scenario suite for
  switch → takeover → failover → restore (PRODUCT-45).
- No benchmarks (criterion) or proxy load harness (PERF-30); only the renderer bundle has a budget.
- `cargo-nextest` is not used: some `#[serial]` tests share fixed temp paths and would race under
  nextest's process-per-test model.
