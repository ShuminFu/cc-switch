# CCS-003: [epic] P2 (v3.20): Typed vocabulary, app registry, IPC contract and generated bindings

**Status:** Done · **Kind:** epic · **Labels:** `epic`, `phase:P2`, `p1-high`, `area:architecture`

> **Ticket key:** `epic-p2` · **Phase:** P2 · **Priority:** p1-high · **Effort:** XL · **Depends on:** `epic-p1`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Goal
Add the shared types that every later move depends on, alongside the existing code, so the compiler checks dispatch before any code moves. Bug-report capture moves to P3 (critique), because CaptureTap needs the P3 stream-tap layer.

## Exit criteria
- [x] openai_responses/openai_chat/gemini_native string literals appear only in format.rs parse/serialize (10 or fewer) — 5 in `proxy/protocol/format.rs`; no other production occurrence in `src-tauri/src`. Remaining hits are `#[cfg(test)]` config fixtures (`proxy/forwarder/wire_golden.rs`, `proxy/trace/tests.rs`, `services/proxy/tests.rs`, `services/provider/mod.rs` tests, `apps/{codex,claude_desktop}/config.rs` tests) and one doc comment in `proxy_harness.rs`.
- [x] #[allow(dead_code)] in proxy/ goes from 69 (actual count, per the critique) to 15 or fewer — `grep -rn 'allow(dead_code)' src-tauri/src/proxy | wc -l` = 10 (CCS-111 reached 6; later P3–P5 work added `codec/mod.rs`, `protocol/ir.rs`, `spend_caps.rs` and two `cfg_attr(not(feature = "testkit"))` allows in `server.rs`).
- [x] No hard-coded app list at lib.rs:1754, services/proxy.rs:1308, tray.rs:157 or App.tsx:124; the registry exhaustiveness test passes — sites derive from `apps::registry::APPS` (`with_caps`); `App.tsx` uses `ALL_APP_IDS`/`isAppId` (no `VALID_APPS`). The guard test `former_hard_coded_app_lists_stay_removed` now covers the split `services/proxy/{mod,lifecycle,config,backup,hot_switch,takeover/*}.rs` and `tray/mod.rs`. `cargo test --lib apps::registry`: 17/17 pass, including `every_variant_registered_exactly_once` and `exhaustiveness_check_flags_dummy_app_without_label`.
- [x] At least 50% of commands return IpcError; errorUtils.ts has no Chinese-substring matching — `node scripts/count-ipc-error-commands.mjs`: 302/302 (100.0%). `src/utils/errorUtils.ts` contains no CJK characters and switches on `getIpcErrorCode` (CCS-115).
- [x] bindings.ts is generated and dirty-checked in CI (or the ts-rs fallback decision is recorded); raw invoke() outside the bindings/api layer is an ESLint error — `src/shared/bindings.ts` is tauri-specta generated (ADR `docs/dev/adr/bindings.md`); `.github/workflows/ci.yml` step "Check generated bindings" regenerates and fails on `git diff` (Linux runner). `cargo test --lib bindings`: 2/2 pass (`bindings_are_up_to_date`). `no-restricted-imports`/`no-restricted-syntax` at `error` outside `src/lib/api/**` and `bindings.ts`; 6 legacy importers remain in `eslint-suppressions.json` (see closure audit).
- [x] Frontend app-literal comparisons are at 200 or fewer — `cc-switch/no-app-literal-comparison` baseline total is 142 (`APP_LITERAL_BUDGET = 142` in `tests/lint/eslintConfig.test.ts`); `pnpm lint` clean, `vitest run tests/lint` 28/28.

## Child tickets
p2-proxy-format-types, p2-proxy-dead-code-purge, p2-app-registry, p2-app-registry-ipc, p2-ipc-error-type, p2-ipc-error-frontend, p2-bindings-spike, p2-bindings-generate, p2-ipc-naming-orphans, p2-fe-app-registry, p2-eslint-ratchets, p2-deps-modernize, p2-feature-actionable-errors

## Epic closure audit

Audited 2026-10-30 on `claude/funny-carson-9c7j3e` (after the P5 merge and CCS-212) against current code and the child tickets' notes.

| Ticket | Key | Status |
| --- | --- | --- |
| CCS-110 | p2-proxy-format-types | Done |
| CCS-111 | p2-proxy-dead-code-purge | Done |
| CCS-112 | p2-app-registry | Done |
| CCS-113 | p2-app-registry-ipc | Done |
| CCS-114 | p2-ipc-error-type | Done |
| CCS-115 | p2-ipc-error-frontend | Done |
| CCS-116 | p2-bindings-spike | Done |
| CCS-117 | p2-bindings-generate | Done |
| CCS-118 | p2-ipc-naming-orphans | Done |
| CCS-119 | p2-fe-app-registry | Partial |
| CCS-120 | p2-eslint-ratchets | Done |
| CCS-121 | p2-deps-modernize | Done |
| CCS-122 | p2-feature-actionable-errors | Done |

**Done.** All six exit criteria hold on the current code:
- Wire-format literals: 5 in `format.rs`, none in other production code.
- `#[allow(dead_code)]` under `proxy/`: 10 (limit 15).
- App lists come from the registry; `cargo test --lib apps::registry` 17/17 pass, with the guard test updated for the `services/proxy/` and `tray/` splits.
- IpcError: 302/302 commands (100%); `errorUtils.ts` has no Chinese text.
- Bindings: tauri-specta generated, `bindings_are_up_to_date` passes, CI regenerates and diff-checks them; raw `invoke` is an ESLint error outside the API layer.
- App-literal comparisons: 142 (limit 200); `pnpm lint` clean; `node scripts/check-ipc-orphans.mjs` reports 298 commands, 0 orphans.

**Residuals (outside the exit criteria, not blocking).**
- CCS-119 is still Partial. Its stated blocker, the backend `get_app_registry`, has since landed (CCS-113; `src/lib/api/apps.ts`, `CapabilityMatrix.tsx`, parity test `registry_dto_matches_shared_fixture`), but `useAppRegistry()` in `src/entities/app/registry.ts` still serves static TS data. Switching it to the backend query is the remaining step; parity is already enforced by the shared fixture, so this does not affect any epic criterion.
- Six files still import `invoke` directly under the ESLint baseline (`DatabaseUpgrade.tsx`, `theme-provider.tsx`, `useProxyConfig.ts`, `useProxyStatus.ts`, `lib/clipboard.ts`, `main.tsx`). The rule is at `error`, so new callers fail; migrating these six onto `src/lib/api`/bindings and running `pnpm lint:prune` is the CCS-120 follow-up.
- The dead-code count rose from 6 (CCS-111) to 10 through later phases; still within the limit.
- The bindings dirty check runs only on the Linux CI runner; it was verified locally here, not in a CI run.
- CCS-122 follow-up: not every backend producer emits the details the actionable-error buttons need (for example `CIRCUIT_OPEN` without `details.appType`); tracked in that ticket.
