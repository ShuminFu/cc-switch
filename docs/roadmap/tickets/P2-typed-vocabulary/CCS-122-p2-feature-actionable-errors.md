# CCS-122: [errors] Add actions to error toasts based on IpcError codes

**Status:** Done · **Kind:** feature · **Labels:** `feature`, `phase:P2`, `p3-low`, `area:frontend`

> **Ticket key:** `p2-feature-actionable-errors` · **Phase:** P2 · **Priority:** p3-low · **Effort:** M · **Depends on:** `p2-ipc-error-frontend`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Error toasts are dead ends. A missing tool, an open circuit or a takeover conflict gives the user no way to act on it.

## Proposed change
Map codes to actions: TOOL_NOT_INSTALLED offers Install, CIRCUIT_OPEN offers Reset, TAKEOVER_ACTIVE offers Stop takeover, and DB_VERSION_TOO_NEW offers Restore backup.

## Acceptance criteria / verification
- [x] Vitest: each mapped code renders its action, and the action calls the right command
- [x] `pnpm test:unit`

## Dependencies
p2-ipc-error-frontend

## Implementation notes

**Summary.** New `getIpcErrorAction(error, t)` (`src/lib/errors/errorActions.ts`) turns an `IpcError` into a sonner toast action, taking the command arguments from `details`:

| Code | Button | Command(s) |
| --- | --- | --- |
| `TOOL_NOT_INSTALLED` (`details.tool`) | Install | `run_tool_lifecycle_action { tools: [tool], action: "install" }` |
| `CIRCUIT_OPEN` (`details.appType`, optional `details.providerId`) | Reset | `reset_circuit_breaker` for that provider, or for every provider in `get_failover_queue(appType)` |
| `TAKEOVER_ACTIVE` (`details.appType`) | Stop takeover | `set_proxy_takeover_for_app { appType, enabled: false }` |
| `DB_VERSION_TOO_NEW` (optional `details.backupId`) | Restore backup | `restore_db_backup` for that backup, or the newest one from `list_db_backups` (the backend takes a safety backup before restoring) |

When a code is unmapped, required context is missing, or the error is a legacy string, the function returns `null` and the toast looks the same as before. Clicking an action shows a follow-up success or failure toast (`errors.actions.*`, in all four locales). The provider-switch toast now shows the fix action instead of Copy when one applies. The proxy-switch toast and the config-import failure toast also pass the action.

`TOOL_NOT_INSTALLED`, `TAKEOVER_ACTIVE` and `DB_VERSION_TOO_NEW` were not in the vocabulary yet. They are now in `ErrorCode` (`src-tauri/src/error.rs`, plus `ALL`, `as_str` and the sync tests), in `IPC_ERROR_CODES` (`src/lib/api/ipc.ts`) and in the regenerated `src/shared/bindings.ts`.

**Follow-up (outside this ticket's acceptance):** no backend command emits the three new codes yet. Emitting them needs these commands to return `IpcError` with the details above: the tool launch/lifecycle commands, a takeover-conflict check, and the schema "database too new" path. Today that path is `AppError::Database`, and `import_config_from_file` / `restore_db_backup` still return `String`. `AllProvidersCircuitOpen` maps to `CIRCUIT_OPEN` but carries no `appType`, so the Reset button only appears once a producer adds `details.appType`.

**Files:** `src/lib/errors/errorActions.ts` (new), `tests/lib/errorActions.test.tsx` (new), `src/lib/query/mutations.ts`, `src/lib/query/proxy.ts`, `src/hooks/useImportExport.ts`, `tests/hooks/useImportExport.test.tsx`, `src/lib/api/ipc.ts`, `src/shared/bindings.ts`, `src-tauri/src/error.rs`, `src/i18n/locales/{en,zh,zh-TW,ja}.json`.

**Commits:** `3e5e92e` (implementation), plus this ticket update.

**Verification:**
- `pnpm vitest run tests/lib/errorActions.test.tsx`: 10/10 passed. For each mapped code, the test renders a real `<Toaster />`, clicks the action button, and checks the exact `invoke` command and args. It also covers the fallbacks, the failure toast, null for unmapped codes, and the switch-mutation wiring.
- `pnpm test:unit`: 154 files, 1500 tests passed.
- `pnpm typecheck`: clean. `pnpm format:check`: clean.
- `cargo fmt`: clean. `cargo clippy -- -D warnings`: clean.
- `cargo test --lib`: 2227 passed, 0 failed, 3 ignored. This includes `error::tests` and `bindings::bindings_are_up_to_date`, run after `UPDATE_BINDINGS=1`.
