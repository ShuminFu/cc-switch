# ADR: Generated TypeScript bindings for Tauri commands

- **Status:** Accepted (spike result, CCS-116)
- **Scope of the spike:** the usage domain (`src-tauri/src/commands/usage.rs`, 13 commands, and its DTOs in `services/usage_stats.rs` and `services/session_usage.rs`, plus `IpcError`/`ErrorCode`).
- **Plan reference:** [next-phase-plan.md](../../roadmap/next-phase-plan.md), Phase 2, "Bindings".

## Context

About 2.9k lines of hand-written TypeScript mirror backend types, and they
drift. Phase 2 needs one generator before deriving types across the other
domains. The two options were:

1. **tauri-specta** (`specta` + `specta-typescript`): it exports types and a typed `commands` object straight from `#[tauri::command]` functions.
2. **ts-rs** (fallback): it exports types only. The command-name map would be generated separately.

`settingsConfig` stays `serde_json::Value` at the boundary in both options.

## Decision

**Adopt tauri-specta as the type generator, pinned to `=2.0.0-rc.25`
(`specta =2.0.0-rc.25`, `specta-typescript =0.0.12`).** The ts-rs fallback is
not triggered. It stays the documented plan B if the specta 2 release line
stalls or the Tauri 3 migration breaks it.

How we run it for now:

- **Test-build only.** The `specta` crates and tauri's `specta` feature are
  `[dev-dependencies]`. DTOs use `#[cfg_attr(test, derive(specta::Type))]`
  and commands use `#[cfg_attr(test, specta::specta)]`. Cargo resolver 2
  keeps dev-dependency features out of normal builds, so the shipped binary
  and `cargo build` are unchanged.
- **The generator is a test.**
  `bindings::bindings_are_up_to_date` (`src-tauri/src/bindings.rs`, moved
  there from `commands::usage` in CCS-117) renders the
  bindings and fails when `src/shared/bindings.ts` is stale. That makes the
  test both the CI dirty check and a compile-time inventory of the registered
  commands. Regenerate with:

  ```bash
  cd src-tauri && UPDATE_BINDINGS=1 cargo test --lib bindings
  ```

- **Types are consumed now. The generated `commands` object is not, yet.**
  `src/types/usage.ts` re-exports and aliases the generated DTO types instead
  of declaring them by hand. `src/lib/api/usage.ts` still calls the backend
  through `src/lib/api/ipc.ts` `invoke`, which normalises rejections into the
  `IpcError` class, so its return types are now the generated ones. The
  runtime handler stays `tauri::generate_handler!`.
- **Drift tests on the frontend.** `tests/lib/usageBindings.test.ts` checks
  two things. First, the `IpcErrorCode` list in `ipc.ts` must equal the
  generated `ErrorCode` union; this is checked at type level, so a mismatch
  fails `pnpm typecheck`. Second, `usageApi` must invoke exactly the command
  names that the generated `commands` object invokes.

## Observed blockers and workarounds

| # | Observation | Impact | Workaround in this spike |
|---|---|---|---|
| 1 | The latest `tauri-specta 2.0.0-rc.26` (published 2026-10-21) expands `collect_commands!` to `__tauri_command_name_<fn>!()`. Only newer `tauri-macros` (2.7.x) emit that helper, but our lock has `tauri 2.10.3` / `tauri-macros 2.5.5`. Every command fails with "cannot find macro `__tauri_command_name_…`". | A hard blocker for rc.26 without a Tauri upgrade. | Pin `rc.25`, which compiles against the current lock. Moving to rc.26+ needs its own Tauri bump ticket. |
| 2 | `serde_json::Value` is an *inline* type in specta, so export fails with "infinitely recursive inline named reference (Value -> Vec<Value> -> Value)". | It affects every DTO that carries a `Value`: `IpcError.details`, and `settingsConfig` on `Provider` when the rollout reaches it. | A field override: `#[cfg_attr(test, specta(type = Option<specta_typescript::Unknown>))]`. That emits `unknown`, which is what "settingsConfig stays Value" means on the TS side. |
| 3 | `u64`/`i64`/`usize` are rejected by default (BigInt-forbidden). | Every counter and every millisecond timestamp. | `Builder::dangerously_cast_bigints_to_number()`. This matches today's wire format (plain JSON numbers). The values in this domain stay far below 2^53. |
| 4 | `f32`/`f64` render as `number \| null`, because serde_json emits `null` for NaN and Infinity. | Consumers such as `successRate` and `cacheHitRate` would have to null-check. | A per-field `#[cfg_attr(test, specta(type = specta_typescript::Number))]`. The backend never produces non-finite rates. |
| 5 | Serde phases: a struct with `skip_serializing_if` that is also `Deserialize` is split into `X_Serialize` / `X_Deserialize` plus a union. `disable_serde_phases()` errors out ("unified mode cannot represent conditional omission"). | `RequestLogDetail` and `PaginatedLogs` get two names each. | The frontend aliases the `_Serialize` shape (`RequestLog = RequestLogDetail_Serialize`), since that is what it receives. |
| 6 | `Option<T>` renders as a required `T \| null` unless serde can omit the field. Command arguments of type `Option<T>` become required positional `T \| null` parameters. | Input DTOs and the generated `commands` signatures are less ergonomic than the hand-written API, which has optional trailing params. | Container-level `#[serde(default)]` on `LogFilters`, which has no runtime change because every field is already `Option`. This made its fields optional (`?`). The generated `commands` object is not used at runtime yet (see the decision). One call site normalises `null` to `undefined` for a query key. |
| 7 | With `ErrorHandlingMode::Result` (the default), commands return `{status, data \| error}`. `Throw` throws the raw payload, not our `IpcError` class. | The generated `commands` cannot replace `src/lib/api/ipc.ts` `invoke` as they are. | Use `Throw` and keep the `ipc.ts` wrapper. To replace it later, use `typed_error_impl` or a thin wrapper around `commands`. |
| 8 | The output is tab-indented and not Prettier-clean. Doc comments keep the leading space from `///`. `Option<Struct>` return types are inlined into the command signature. | `pnpm format:check` would fail. The output is noisier to read. | `src/shared/bindings.ts` is listed in `.prettierignore`. The file is owned by the generator, and the dirty check guards it. |
| 9 | Every command needs `#[specta::specta]`, and every reachable type needs `Type` derived on it transitively. | The full rollout touches every DTO, which is mechanical but wide. | None needed. This is expected rollout work. |
| 10 | Tauri 3 is in alpha (`3.0.0-alpha.5`), and specta 2 is still a release candidate that needs exact `=` pins. | Upgrades are lock-step. | Exact pins. Revisit when Tauri 3 or specta 2.0 stable ships. |

## Consequences

- The usage domain's DTO interfaces are no longer hand-written: 120 lines were
  removed from `src/types/usage.ts`. If backend DTOs or `ErrorCode` drift, the
  Rust dirty check fails, then `pnpm typecheck` fails.
- Command names are covered by a frontend test against the generated
  `commands`. They are not yet a compile-time map.
- The release build has no new dependencies. Binding generation needs
  `cargo test` (debug, test cfg).

## CCS-117 rollout

- `Type` is now also derived (test cfg only) on `AppType`, `Provider` (with
  `ProviderMeta` and its nested DTOs; `settingsConfig` and
  `localProxyRequestOverrides.body` export as `unknown`), `ProxyStatus` and
  `ActiveTarget`. They are registered with `Builder::typ` because their
  commands do not carry `#[specta::specta]` yet.
- The builder also exports `COMMANDS`, every command name in the
  per-domain registry `src-tauri/src/commands/registry.rs` that also feeds
  `generate_handler!` and the ACL permission sets (CCS-180). Adding or removing a command therefore also makes
  the committed bindings stale.
- CI: the Linux `Backend Checks` job has a `Check generated bindings` step. It
  regenerates the file and fails with a `git diff` when it differs from the
  committed one. The full `cargo test` run on every OS also fails on a stale
  file.
- `src/types/proxy.ts` re-exports the generated `ProxyStatus`/`ActiveTarget`.
- MSW: `tests/msw/handlers.ts` registers mocks through `command(name, resolve)`.
  `name` must be a generated `CommandName` and `resolve` must return the
  generated result type (`tests/msw/commands.ts`), or an `HttpResponse` to
  simulate a failure. `tests/msw/handlers.test.ts` requires every registered
  command to have a handler or to be listed in
  `tests/msw/unmockedCommands.ts`, rejects handlers for unregistered commands,
  and checks `AppId` equals the generated `AppType_Serialize`.
- Observed drift fixed by the typing: the backend always serialises
  `Provider.inFailoverQueue` (it is `#[serde(default)] bool`), while the
  frontend type has it optional. The `get_providers` mock now fills it in.
  Five mocks for commands that no longer exist were removed
  (`import_mcp_from_claude`, `import_mcp_from_codex`,
  `select_config_directory`, `reorder_failover_queue`,
  `set_failover_item_enabled`).

## Follow-ups (not part of the spike)

1. Roll `Type`/`specta::specta` out to the remaining domains (CCS-117 covered the Provider, AppType and ProxyStatus DTOs, but not their commands). Then grow `src/shared/bindings.ts` from a single builder that lists every command (Phase 5 `collect_commands!` split).
2. ~~Run the bindings dirty check in CI as its own step.~~ Done in CCS-117.
3. Decide whether to switch the runtime handler to `Builder::invoke_handler()` and use the generated `commands` with an `IpcError`-normalising `typed_error_impl`. The camelCase command names were normalised to `query_provider_usage` and `test_usage_script` in CCS-118; `scripts/check-ipc-orphans.mjs` keeps command names snake_case.
4. Bump Tauri to a version whose `tauri-macros` emits `__tauri_command_name_*`, then move to tauri-specta rc.26+.
5. ~~Type `tests/msw/handlers.ts` against the generated types.~~ Done in CCS-117. The remaining step is to add `#[specta::specta]` to the provider and proxy commands so `ResultOverrides` in `tests/msw/commands.ts` can go away.
