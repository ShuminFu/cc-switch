# CCS-117: [ipc] Generate src/shared/bindings.ts with a CI dirty check and typed MSW handlers

**Status:** Done · **Kind:** refactor · **Labels:** `refactor`, `phase:P2`, `p1-high`, `area:ipc`

> **Ticket key:** `p2-bindings-generate` · **Phase:** P2 · **Priority:** p1-high · **Effort:** L · **Depends on:** `p2-bindings-spike`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Frontend and backend types drift, and MSW handlers are untyped.

## Proposed change
- Derive Type on Provider, AppType, ProxyStatus, IpcError and the usage DTOs.
- Generate bindings.ts in a build step, and fail CI if the generated file differs from the committed one.
- Type tests/msw/handlers.ts against it, and add a test that every command has a handler or is allowlisted.

## Acceptance criteria / verification
- [x] CI fails when a Rust DTO changes without regenerating
- [x] `pnpm typecheck` and `pnpm test:unit`

## Dependencies
p2-bindings-spike

## Implementation notes

**Summary.** This builds on the CCS-116 spike: tauri-specta `=2.0.0-rc.25`, test-build only.
- **Generator.** It moved from `commands::usage` into its own test-only module, `src-tauri/src/bindings.rs`. Its test `bindings::bindings_are_up_to_date` renders `src/shared/bindings.ts` and fails when the committed file is stale. Regenerate with `cd src-tauri && UPDATE_BINDINGS=1 cargo test --lib bindings`.
- **New derives.** `Type` is now also derived (`cfg_attr(test)`) on:
  - `AppType`
  - `Provider`, together with `ProviderMeta`, `UsageScript`, `AuthBinding(Source)`, `ClaudeDesktopMode`, `ClaudeDesktopModelRoute`, `CodexChatReasoningConfig`, `LocalProxyRequestOverrides` and `CustomEndpoint`
  - `ProxyStatus` and `ActiveTarget`

  They are registered with `Builder::typ`. `IpcError` and the usage DTOs were already covered by the spike. `serde_json::Value` fields export as `unknown`, and `ProxyStatus.success_rate` as `number`. None of this has a runtime or release-build effect.
- **Command list.** The bindings now also export `COMMANDS`, every name in the `generate_handler!` list in `lib.rs` (parsed from source). Adding or removing a command therefore also makes the bindings stale.
- **CI.** `.github/workflows/ci.yml` has a new `Check generated bindings` step in the Linux backend job. It regenerates the file, then fails with `git diff --exit-code` and an `::error` annotation if anything changed. The existing `cargo test` step on all three OSes also fails on a stale file.
- **MSW.**
  - `tests/msw/handlers.ts` registers every mock through `command(name, resolve)`. `name` must be a generated `CommandName`, and the resolver must return the generated result type: the usage commands come from the generated `commands`, and `get_providers`, `get_current_provider` and `get_proxy_status` are mapped through `tests/msw/commands.ts`. An `HttpResponse` can still be returned to simulate a failure.
  - The new `tests/msw/handlers.test.ts` checks that every registered command has a handler or is listed in `tests/msw/unmockedCommands.ts` (233 entries, typed `satisfies readonly CommandName[]`). It also checks that there are no handlers for unregistered commands, no stale or duplicate allowlist entries, and that `AppId` equals the generated `AppType_Serialize`.
- **Drift the typing found.**
  - The backend always serialises `Provider.inFailoverQueue`, but the frontend type marks it optional. The `get_providers` mock now fills it in.
  - Five mocks were for commands that no longer exist: `import_mcp_from_claude`, `import_mcp_from_codex`, `select_config_directory`, `reorder_failover_queue` and `set_failover_item_enabled`. They were removed.
- **Frontend types.** `src/types/proxy.ts` now re-exports the generated `ProxyStatus` and `ActiveTarget` instead of hand-written copies.
- **ADR.** `docs/dev/adr/bindings.md` is updated with a "CCS-117 rollout" section and the follow-up list.

**Files:**
- `src-tauri/src/bindings.rs` (new), `src-tauri/src/lib.rs`, `src-tauri/src/commands/usage.rs`, `src-tauri/src/app_config.rs`, `src-tauri/src/provider.rs`, `src-tauri/src/proxy/types.rs`, `src-tauri/src/settings.rs`
- `src/shared/bindings.ts` (generated), `src/types/proxy.ts`
- `tests/msw/handlers.ts`, `tests/msw/commands.ts` (new), `tests/msw/unmockedCommands.ts` (new), `tests/msw/handlers.test.ts` (new)
- `.github/workflows/ci.yml`, `docs/dev/adr/bindings.md`

**Commits:** 2f148da (implementation), plus this ticket-notes commit.

**Verification:**
- `cargo fmt`: clean.
- `cargo clippy -- -D warnings` and `cargo clippy --lib --tests -- -D warnings`: clean.
- `cargo test --lib bindings`: 2 passed. Changing a DTO doc comment on `ActiveTarget` without regenerating makes `bindings_are_up_to_date` fail ("src/shared/bindings.ts is stale").
- CI step simulated locally (regenerate, then `git diff --exit-code`): it passes on the committed tree. After the same DTO change, it reports `src/shared/bindings.ts | 2 +-` and fails.
- `cargo test --no-fail-fast`:
  - lib: 2224 passed / 0 failed / 3 ignored.
  - Every integration binary passed except `tests/migrations.rs::every_released_schema_migrates_to_current`. That failure is the known time-dependent flake already noted in CCS-116 (a fixture row in `proxy_request_logs`). It passed 3 of 3 standalone reruns, and it does not compile any of the test-cfg code this ticket touches.
- `pnpm typecheck`: passes. Two deliberate breakages were caught: a typo'd handler command name, and `success_rate: "0"` in the `get_proxy_status` mock.
- `pnpm test:unit`: 151 files / 1457 tests passed.
- `pnpm format:check`: clean.
- `eslint tests/msw src/types/proxy.ts`: clean.
- The edited `ci.yml` parses with `yaml.safe_load`. actionlint is not installed here.

**Remaining (follow-ups, not blocking):**
- The provider and proxy commands do not carry `#[specta::specta]` yet, so their result types are mapped by hand in `ResultOverrides` in `tests/msw/commands.ts`.
- The rest of the hand-written frontend `Provider` type is not replaced yet.

**Review.** An independent review checked both acceptance criteria against 2f148da and found them met. Regenerating three times gave byte-identical output. A probe doc-comment change on `ActiveTarget` made `bindings_are_up_to_date` fail, and reverting it made the test pass again. No frontend `invoke("…")` call names a command missing from the generated `COMMANDS`. Re-run results:
- `cargo fmt --check`: clean.
- `cargo clippy --all-targets -- -D warnings`: clean.
- `cargo test --lib`: 2224 passed.
- `cargo test --no-fail-fast`: 0 failures.
- `pnpm typecheck`, `pnpm test:unit` (1457 tests) and `pnpm format:check`: all pass.
- `ci.yml` parses as valid YAML.

The one follow-up fixed: the `.prettierignore` comment still named the removed `usage_bindings` test, and now names the `bindings` test.
