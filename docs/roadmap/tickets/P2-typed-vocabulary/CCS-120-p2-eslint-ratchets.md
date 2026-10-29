# CCS-120: [lint] Ban raw invoke outside the bindings layer and ratchet app-literal comparisons

**Status:** Done · **Kind:** chore · **Labels:** `chore`, `phase:P2`, `p2-medium`, `area:frontend`

> **Ticket key:** `p2-eslint-ratchets` · **Phase:** P2 · **Priority:** p2-medium · **Effort:** S · **Depends on:** `p2-bindings-generate`, `p2-fe-app-registry`, `p0-ci-eslint-prettier`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Nothing stops new code from bypassing the bindings or adding app-id literal comparisons (283 today).

## Proposed change
- Make no-restricted-imports for `invoke` an error outside src/shared/bindings.ts and src/lib/api.
- Add a custom rule or script that counts app-literal comparisons, with a baseline file.

## Acceptance criteria / verification
- [x] A new raw invoke fails `pnpm lint`
- [x] The literal count is 200 or fewer and enforced

## Dependencies
p2-bindings-generate, p2-fe-app-registry, p0-ci-eslint-prettier

## Implementation notes

**Summary.** Two ESLint ratchets for `src/`. Both rules are at `error`. Existing violations are recorded in `eslint-suppressions.json` (the bulk-suppression baseline), so `pnpm lint` fails only on new ones.

- **Raw `invoke`.** `no-restricted-imports` bans importing `invoke` from `@tauri-apps/api/core` (named or namespace import) everywhere in `src/` except `src/lib/api/**` and `src/shared/bindings.ts`. That file does not exist yet because `p2-bindings-generate` has not landed; it is allowed ahead of time. The 7 existing direct importers (`App.tsx`, `main.tsx`, `DatabaseUpgrade.tsx`, `theme-provider.tsx`, `useProxyConfig.ts`, `useProxyStatus.ts`, `lib/clipboard.ts`) are in the baseline. A test fails if any other file joins it.
- **App-id literals.** A new local rule, `cc-switch/no-app-literal-comparison` (`eslint-rules/index.js`), reports `==`/`===`/`!=`/`!==` comparisons against an app-id string literal, and `case "<app>":` labels. It hard-codes the app-id list; a test checks that list against `ALL_APP_IDS` from the registry. Measured with this rule, `src/` had 342 such comparisons at the start, and the plan's grep measured 283. After this change there are **198**. `tests/lint/eslintConfig.test.ts` caps the baselined total at `APP_LITERAL_BUDGET = 198`, which must stay at or below 200 and should only go down.
- **How the count went from 342 to 198.** Each change keeps the same behaviour:
  - `ProviderFormFull` compares `appId` against each app once (`isClaudeApp`, `isCodexApp`, and so on) and branches on those values. Hook dependency lists name the booleans in place of `appId`.
  - The `opencode || openclaw || hermes` chains in `App.tsx`, `AddProviderDialog`, `ProviderCard` and `mutations.ts` now use the registry's `additive` capability (`useAppCaps` / `appCaps`).
  - `useOpencodeFormState`, `useOpenclawFormState` and `useHermesFormState` each compare `appId` once.
  - `useApiKeyLink`: the 7-app allow-list (every app except grokbuild) is now `appId !== "grokbuild"`.
  - `useProviderCategory` checks `type !== appId` once, instead of `type === X && appId === X` in every branch.

**Files:** `eslint.config.js`, `eslint-rules/index.js` (new), `eslint-rules/index.d.ts` (new), `eslint-suppressions.json`, `tests/lint/eslintConfig.test.ts`, `src/App.tsx`, `src/components/providers/AddProviderDialog.tsx`, `src/components/providers/ProviderCard.tsx`, `src/components/providers/forms/ProviderForm.tsx`, `src/components/providers/forms/hooks/{useApiKeyLink,useHermesFormState,useOpenclawFormState,useOpencodeFormState,useProviderCategory}.ts`, `src/lib/query/mutations.ts`.

**Commits:** `b60f594` (code, tests, baseline and these notes); this SHA was filled in by a follow-up docs commit.

**Verification.**
- `pnpm lint`: clean.
- Probe: a new file `src/__probe__.ts` with `import { invoke } from "@tauri-apps/api/core"` and `a === "codex"` made `pnpm lint` fail with 2 errors (`no-restricted-imports` and `cc-switch/no-app-literal-comparison`). The probe was then removed.
- `npx vitest run tests/lint`: 12/12 pass. The new tests cover a raw invoke in a new file, a namespace import, the API-layer exemption, app-literal comparisons including a `switch` case, registry and non-app literals being accepted, rule/registry id parity, the budget cap, and the invoke baseline allow-list.
- Regression check: with `eslint.config.js` restored to HEAD, 3 of the new tests fail (raw invoke, namespace import, app literals).
- `pnpm typecheck`: clean. `pnpm format:check`: clean. `pnpm test:unit`: 110 files, 1182 tests passed.

**Follow-ups (outside this ticket's acceptance list).** Once `p2-bindings-generate` lands, move the 7 baselined `invoke` callers onto the bindings and run `pnpm lint:prune`. The plan's P3 target is 40 or fewer app-literal comparisons.
