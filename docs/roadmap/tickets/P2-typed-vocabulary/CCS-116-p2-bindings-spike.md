# CCS-116: [ipc] Time-boxed tauri-specta spike on the usage domain, with a written decision

**Status:** Done · **Kind:** chore · **Labels:** `chore`, `phase:P2`, `p1-high`, `area:ipc`

> **Ticket key:** `p2-bindings-spike` · **Phase:** P2 · **Priority:** p1-high · **Effort:** S · **Depends on:** `p2-ipc-error-type`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
About 2.9k lines of hand-written TS mirror backend types and drift. Whether tauri-specta fits Tauri 2 and the existing serde shapes is unknown.

## Proposed change
- Spend one week generating bindings for the usage domain with tauri-specta.
- Write docs/dev/adr/bindings.md recording the choice. The fallback is ts-rs plus a generated command-name map.
- settingsConfig stays Value at the boundary.

## Acceptance criteria / verification
- [x] The ADR is merged with the decision and the observed blockers
- [x] The usage domain compiles against the generated types: `pnpm typecheck`

## Dependencies
p2-ipc-error-type

## Implementation notes

**Summary.** The spike generated TypeScript bindings for the 13 usage-domain commands with tauri-specta. The decision is to **adopt tauri-specta, pinned to `=2.0.0-rc.25`**. The ts-rs fallback was not needed and stays documented as plan B. The decision and ten observed blockers, each with its workaround, are in `docs/dev/adr/bindings.md`.

The two main blockers:
- rc.26 requires a newer `tauri-macros` than our lock has (2.5.5), because it uses the `__tauri_command_name_*` helper macros.
- `serde_json::Value` hits an inline-recursion export error. The workaround is to export it as `unknown`, which also keeps `settingsConfig` as `Value` at the boundary.

How it is wired:
- The specta crates are dev-dependencies, and the derives/attributes are behind `cfg_attr(test, …)`. The release build and the runtime `generate_handler!` are unchanged.
- The Rust test `commands::usage::usage_bindings::usage_bindings_are_up_to_date` renders the bindings and fails when `src/shared/bindings.ts` is stale. That makes it the dirty check. Regenerate with `UPDATE_BINDINGS=1 cargo test --lib usage_bindings`.
- `src/types/usage.ts` now re-exports and aliases the generated DTO types. 120 hand-written lines were removed. `usageApi` keeps the `ipc.ts` `invoke` wrapper for `IpcError` normalisation, but its types are now the generated ones.
- `tests/lib/usageBindings.test.ts` checks two things. At type level, `IpcErrorCode` must equal the generated `ErrorCode`; a mismatch fails `pnpm typecheck`, which I confirmed by adding a fake code. At runtime, `usageApi` must invoke exactly the generated command names.

Backend changes with no runtime effect:
- `LogFilters` gets container-level `#[serde(default)]`. All its fields are `Option` already, so its TS fields come out as optional.
- Float rate fields get a test-only `specta(type = Number)` override.
- `IpcError.details` gets a test-only `specta(type = Unknown)` override.

**Files:**
- `src-tauri/Cargo.toml`, `src-tauri/Cargo.lock`
- `src-tauri/src/commands/usage.rs`, `src-tauri/src/error.rs`, `src-tauri/src/services/usage_stats.rs`, `src-tauri/src/services/session_usage.rs`
- `src/shared/bindings.ts` (generated), `src/types/usage.ts`, `src/lib/query/usage.ts`
- `tests/lib/usageBindings.test.ts`
- `.prettierignore` (excludes the generated file)
- `docs/dev/adr/bindings.md`

**Commits:** f621cca (implementation and ADR), plus this ticket-notes commit.

**Verification:**
- `cargo fmt`: clean.
- `cargo clippy -- -D warnings`: clean. `cargo clippy --lib --tests -- -D warnings` is also clean.
- `cargo test --lib usage_bindings`: passes. With a deliberately edited `bindings.ts` it fails with "src/shared/bindings.ts is stale".
- `cargo test --no-fail-fast`: lib 2223 passed / 0 failed / 3 ignored, and every integration test binary passed except one flaky failure. `tests/migrations.rs::every_released_schema_migrates_to_current` failed once (v9 fixture row in `proxy_request_logs`). It then passed on 2 of 3 standalone reruns. It does not touch any code this ticket changed, which is test-cfg only, so it is treated as a pre-existing flake.
- `pnpm typecheck`: passes.
- `pnpm test:unit`: 150 files / 1453 tests passed.
- `pnpm format:check`: clean.
- `eslint` on the touched TS files: clean.

**Follow-ups (in the ADR):**
- Roll the derives out to Provider, AppType and ProxyStatus.
- Add a dedicated CI dirty-check step.
- Decide whether to switch the runtime to `Builder::invoke_handler()` and use the generated `commands`.
- Bump Tauri, then move to tauri-specta rc.26+.
- Type the MSW handlers against the generated types.
