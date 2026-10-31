# CCS-003: [epic] P2 (v3.20): Typed vocabulary, app registry, IPC contract and generated bindings

**Status:** Done · **Kind:** epic · **Labels:** `epic`, `phase:P2`, `p1-high`, `area:architecture`

> **Ticket key:** `epic-p2` · **Phase:** P2 · **Priority:** p1-high · **Effort:** XL · **Depends on:** `epic-p1`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Goal
Add the shared types that every later move depends on, alongside the existing code, so the compiler checks dispatch before any code moves. Bug-report capture moves to P3 (critique), because CaptureTap needs the P3 stream-tap layer.

## Exit criteria
- [x] openai_responses/openai_chat/gemini_native string literals appear only in format.rs parse/serialize (10 or fewer) — 5 in `proxy/protocol/format.rs`; no other production occurrence in `src-tauri/src`. Remaining hits are `#[cfg(test)]` config fixtures (`proxy/forwarder/wire_golden.rs`, `proxy/trace/tests.rs`, `services/proxy/tests.rs`, `services/provider/mod.rs` tests, `apps/{codex,claude_desktop}/config.rs` tests) and one doc comment in `proxy_harness.rs`.
- [x] #[allow(dead_code)] in proxy/ goes from 69 (actual count, per the critique) to 15 or fewer — `grep -rn 'allow(dead_code)' src-tauri/src/proxy | wc -l` = 10 (CCS-111 reached 6; later P3–P5 work added `codec/mod.rs`, `protocol/ir.rs`, `spend_caps.rs` and two `cfg_attr(not(feature = "testkit"))` allows in `server.rs`).
- [x] No hard-coded app list at lib.rs:1754, services/proxy.rs:1308, tray.rs:157 or App.tsx:124; the registry exhaustiveness test passes — sites derive from `apps::registry::APPS` (`with_caps`); `App.tsx` has no `VALID_APPS`; the frontend `useAppRegistry()` now reads the backend `get_app_registry` (CCS-229). The guard test `former_hard_coded_app_lists_stay_removed` now covers the split `services/proxy/{mod,lifecycle,config,backup,hot_switch,takeover/*}.rs` and `tray/mod.rs`. `cargo test --lib apps::registry`: 17/17 pass, including `every_variant_registered_exactly_once` and `exhaustiveness_check_flags_dummy_app_without_label`.
- [x] At least 50% of commands return IpcError; errorUtils.ts has no Chinese-substring matching — `node scripts/count-ipc-error-commands.mjs`: 302/302 (100.0%). `src/utils/errorUtils.ts` contains no CJK characters and switches on `getIpcErrorCode` (CCS-115).
- [x] bindings.ts is generated and dirty-checked in CI (or the ts-rs fallback decision is recorded); raw invoke() outside the bindings/api layer is an ESLint error — `src/shared/bindings.ts` is tauri-specta generated (ADR `docs/dev/adr/bindings.md`); `.github/workflows/ci.yml` step "Check generated bindings" regenerates and fails on `git diff` (Linux runner). `cargo test --lib bindings`: 2/2 pass (`bindings_are_up_to_date`). `no-restricted-imports`/`no-restricted-syntax` at `error` outside `src/lib/api/**` and `bindings.ts`; 6 legacy importers remain baselined in `eslint-suppressions.json` (see closure audit).
- [x] Frontend app-literal comparisons are at 200 or fewer — `cc-switch/no-app-literal-comparison` baseline total is 27 (`APP_LITERAL_BUDGET = 27` in `tests/lint/eslintConfig.test.ts`, which now requires baseline == budget and budget ≤ 40; lowered from 142 by CCS-251/252); `pnpm lint` clean.

## Child tickets
p2-proxy-format-types, p2-proxy-dead-code-purge, p2-app-registry, p2-app-registry-ipc, p2-ipc-error-type, p2-ipc-error-frontend, p2-bindings-spike, p2-bindings-generate, p2-ipc-naming-orphans, p2-fe-app-registry, p2-eslint-ratchets, p2-deps-modernize, p2-feature-actionable-errors

Follow-ups: p2-fe-app-registry-from-backend (CCS-229); app-literal reductions in Track F (CCS-251, CCS-252).

## Epic closure audit

Re-audited 2026-10-31 on `claude/funny-carson-9c7j3e` at `673be73` (after the follow-up wave: CCS-214..246 on this branch plus the lane 2 merge of CCS-213..253) against current code and ticket notes. Replaces the 2026-10-30 audit.

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
| CCS-119 | p2-fe-app-registry | Done (closed by CCS-229) |
| CCS-120 | p2-eslint-ratchets | Done |
| CCS-121 | p2-deps-modernize | Done |
| CCS-122 | p2-feature-actionable-errors | Done |
| CCS-229 | p2-fe-app-registry-from-backend (follow-up) | Done |
| CCS-251 | f-lint-app-literals-batch1 (Track F, lowers this epic's literal count) | Done |
| CCS-252 | f-lint-app-literals-batch2 (Track F) | Done |

**Done.** All six exit criteria hold, re-measured on the current code:
- Wire-format literals: 5 in `proxy/protocol/format.rs`; every other hit in `src-tauri/src` is a `#[cfg(test)]` fixture or the doc comment in `proxy_harness.rs`.
- `#[allow(dead_code)]` under `proxy/`: 10 (limit 15). Unchanged by the follow-ups (CCS-230/245/246 did not add any).
- App lists: `cargo test --lib apps::registry` 17/17 pass, including `every_variant_registered_exactly_once`, `exhaustiveness_check_flags_dummy_app_without_label` and `former_hard_coded_app_lists_stay_removed`. CCS-229 closed the old CCS-119 residual: `useAppRegistry()` queries `get_app_registry` and merges it over the static table (`tests/entities/appRegistry.test.tsx` passes with the per-app fixture parity and the MSW hook test).
- IpcError: `node scripts/count-ipc-error-commands.mjs` 302/302 (100.0%); `src/utils/errorUtils.ts` has 0 CJK characters and switches on `getIpcErrorCode`.
- Bindings: `cargo test --lib bindings` 2/2 pass (`bindings_are_up_to_date`); `.github/workflows/ci.yml` step "Check generated bindings" regenerates and diff-checks on Linux. `node scripts/check-ipc-orphans.mjs`: 298 commands, 0 orphans, 0 unknown, 0 non-snake_case. Raw `invoke` is an ESLint error outside `src/lib/api/**` and `bindings.ts`.
- App-literal comparisons: 27 (limit 200; Track F target 40 also met). `pnpm lint` exits 0; `vitest run tests/lint tests/entities/appRegistry.test.tsx` 69/69.

**What remains.** Nothing that needs CI, other machines, settings, a tag or a GUI check is required for any exit criterion; the bindings dirty check has been verified locally, and its first green run on a GitHub Linux runner is the only CI-side confirmation outstanding.

**Residuals (outside the exit criteria, not blocking).**
- Six files still import `invoke` directly under the ESLint baseline (`DatabaseUpgrade.tsx`, `theme-provider.tsx`, `useProxyConfig.ts`, `useProxyStatus.ts`, `lib/clipboard.ts`, `main.tsx`). New callers fail lint; moving these six onto `src/lib/api`/bindings and running `pnpm lint:prune` is the open CCS-120 follow-up (no ticket filed yet). Local code work, not a CI or machine gate.
- CCS-122 follow-up: backend producers still do not attach the details the action buttons need (for example `CIRCUIT_OPEN` from `AllProvidersCircuitOpen` has no `details.appType`, and `TOOL_NOT_INSTALLED`/`TAKEOVER_ACTIVE`/`DB_VERSION_TOO_NEW` have no emitters). Local code work.
- The ticket index `docs/roadmap/tickets/README.md` still lists CCS-119 as Partial and CCS-229 as Open; both ticket files say Done. The index row needs updating by whoever owns the README.
