# CCS-211: [lint] Make pnpm lint green again after the P1/P2 merges

**Status:** Open · **Kind:** chore · **Labels:** `chore`, `phase:F`, `p1-high`, `area:frontend`

> **Ticket key:** `f-lint-green` · **Phase:** F · **Priority:** p1-high · **Effort:** S
> Plan: [next-phase-plan.md](../../next-phase-plan.md)
> Found while verifying the lane-2 merge (348c874). The failure already existed on both merge parents.

## Problem
`pnpm lint` reports 20 errors, so the CI lint gate from CCS-017/CCS-120 is red:
- 19 × `cc-switch/no-app-literal-comparison` in `src/App.tsx` (81, 147, 348), `src/components/DeepLinkImportDialog.tsx` (340-723) and `src/components/providers/ProviderActions.tsx` (95-267). Code added in P1 and the frontend track compares app ids against string literals, and the suppression baseline `eslint-suppressions.json` holds lower counts than the code now has. ESLint also warns that some suppressions no longer match anything.
- 1 × `react-hooks/exhaustive-deps` at `DeepLinkImportDialog.tsx:175`: `requestSeqRef.current` is read in an effect cleanup (introduced by CCS-071).

## Proposed change
- Replace the literal comparisons with registry lookups (`appCaps`, `appsWithCap`, `isAppId`, or new capabilities in `src/entities/app/registry.ts`, such as `additive` or `deeplinkEnvConfig`). Do not raise the suppression counts.
- Fix the cleanup ref read by copying the ref into a local variable inside the effect, keeping the CCS-071 stale-request guard semantics.
- Prune stale suppressions (`--prune-suppressions`), so the baseline only goes down.

## Acceptance criteria / verification
- [ ] `pnpm lint` exits 0 with no "suppressions left" warning.
- [ ] Total suppressed `no-app-literal-comparison` count does not increase relative to `eslint-suppressions.json` at 348c874.
- [ ] `pnpm typecheck`, `pnpm test:unit` and `pnpm format:check` pass, including the existing DeepLinkImportDialog stale-request tests from CCS-071.

## Dependencies
None.
