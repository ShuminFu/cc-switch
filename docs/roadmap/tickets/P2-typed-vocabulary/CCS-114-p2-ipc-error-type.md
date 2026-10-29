# CCS-114: [ipc] Introduce IpcError with stable error codes

**Status:** Done · **Kind:** refactor · **Labels:** `refactor`, `phase:P2`, `p2-medium`, `area:ipc`

> **Ticket key:** `p2-ipc-error-type` · **Phase:** P2 · **Priority:** p2-medium · **Effort:** M
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Commands return String errors, and the frontend matches Chinese substrings.

## Evidence
- error.rs:125 format_skill_error; AppError::Localized

## Proposed change
- Add `IpcError{code: ErrorCode, message, details, i18n_key}` with From<AppError> (keeping Localized.key) and a temporary From<String> that maps to UNKNOWN.
- Replace format_skill_error.
- Migrate the skills, proxy and provider domains first.

## Acceptance criteria / verification
- [x] Serialization test for every ErrorCode
- [x] A count script reports at least 50% of commands returning IpcError
- [x] `cargo test --manifest-path src-tauri/Cargo.toml error`

## Dependencies
None.

## Implementation notes

**Summary**
- `src-tauri/src/error.rs`: new `ErrorCode` (23 variants, serialized as SCREAMING_SNAKE_CASE, `ErrorCode::ALL` + `as_str()`) and `IpcError { code, message, details, i18n_key }` (wire: `{code, message, details, i18nKey}`). `From<AppError>` maps every variant to a code and keeps `Localized.key` as `i18nKey` (zh/en in `details`); `From<anyhow::Error>` recovers a wrapped `IpcError` or `AppError`; `From<String>`/`From<&str>`/`From<rusqlite::Error>` are the transitional conversions (`String` → `UNKNOWN`). `message` stays the old human-readable text. `details` is boxed to keep `Result<T, IpcError>` small (clippy `result_large_err`).
- `format_skill_error` removed; skills services/commands build `IpcError::skill(ErrorCode::…, context, suggestion)` with `details = {context, suggestion}`.
- Migrated commands to `Result<_, IpcError>`: skills (24), proxy (24), provider (28), failover (6), global proxy (3), stream check (4), usage (13), settings (19), MCP (14), config (14), OpenClaw (14). `.map_err(|e| e.to_string())` became `.map_err(IpcError::from)` wherever the source is `AppError`/anyhow, so codes are preserved.
- Frontend: `src/lib/api/ipc.ts` wraps `invoke` (all `src/lib/api/*` modules now import it) and turns a structured rejection into an `IpcError` (`code`, `details`, `i18nKey`; `toString()` returns the message so existing `String(error)` toasts are unchanged); legacy string rejections pass through. `skillErrorParser.ts` switches on `code` instead of parsing JSON strings; SkillsPage and the ZIP-install/update toasts in UnifiedSkillsPanel use it. DatabaseUpgrade uses `extractErrorMessage` for the migrated `install_update_and_restart`.
- `scripts/count-ipc-error-commands.mjs [--min <pct>]` counts `#[tauri::command]` fns returning `IpcError`: **163/286 (57.0%)**.
- Not done here (out of this ticket's acceptance list, tracked by the phase exit criteria): MCP Chinese-substring matching in `errorUtils.ts` (`translateMcpBackendError`) still matches text, because MCP validation errors are still `AppError::McpValidation(String)` without per-case codes; the remaining 123 commands still return `String`.

**Files**
- Rust: `src-tauri/src/error.rs`, `src-tauri/src/services/skill.rs`, `src-tauri/src/commands/{skill,proxy,provider,failover,global_proxy,stream_check,usage,settings,mcp,config,openclaw}.rs`
- Web: `src/lib/api/ipc.ts` (new), `src/lib/api/*.ts` (import swap), `src/lib/errors/skillErrorParser.ts`, `src/components/skills/{SkillsPage,UnifiedSkillsPanel}.tsx`, `src/components/DatabaseUpgrade.tsx`
- Script/tests: `scripts/count-ipc-error-commands.{mjs,d.mts}`, `tests/scripts/countIpcErrorCommands.test.ts`, `tests/lib/ipcError.test.ts`

**Commits**
- `0a6ee20` refactor(ipc): introduce IpcError with stable error codes (CCS-114)

**Verification**
- `cargo test --manifest-path src-tauri/Cargo.toml error` → all ok (lib: 99 passed incl. `error_code_serializes_to_stable_string_for_every_variant`, `error_code_all_lists_every_variant_once`, `ipc_error_from_*`).
- Regression: `commands::skill::install_from_zip_path_tests::install_accepts_zip_path_chosen_by_dialog` now asserts `err.code == ErrorCode::NoSkillsInZip` (before, the helper returned a JSON `String`).
- `cargo test --manifest-path src-tauri/Cargo.toml --no-fail-fast` → 2380 passed, 0 failed.
- `cargo fmt` clean; `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings` → clean.
- `node scripts/count-ipc-error-commands.mjs --min 50` → `IpcError commands: 163/286 (57.0%)`, exit 0.
- `pnpm typecheck` → ok; `pnpm format:check` → ok; `pnpm test:unit` → 149 files / 1451 tests passed.
