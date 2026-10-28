# CCS-020: [i18n] Add a locale parity and used-key check to CI

**Status:** Done · **Kind:** chore · **Labels:** `chore`, `phase:P0`, `p2-medium`, `area:i18n`

> **Ticket key:** `p0-ci-i18n-parity` · **Phase:** P0 · **Priority:** p2-medium · **Effort:** S
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Missing and extra keys reach users. 10 keys used in code exist in no locale, and zh-TW lacks 30.

## Evidence
- src/i18n/locales/{en,ja,zh,zh-TW}.json; src/i18n/index.ts:82 fallbackLng

## Proposed change
- Add a vitest test (or a scripts/check-i18n.mjs run in CI) that:
  - flattens all 4 locales and asserts identical key sets
  - extracts literal t("...") keys from src/ and asserts each exists in en.json
- Allowlist dynamic keys explicitly.

## Acceptance criteria / verification
- [x] The test fails on the current tree, listing the 10 missing and 30 zh-TW keys, and passes after the two key-fill tickets
- [x] `pnpm test:unit`

## Dependencies
None (lands together with p0-i18n-missing-keys and p0-i18n-zhtw-keys).

## Implementation notes

**Summary.** The locale parity half (all 4 locales flattened, identical key sets and interpolation variables vs en) already landed with CCS-022 as `tests/i18n/localeParity.test.ts`. This ticket adds `tests/i18n/usedKeys.test.ts`, which scans every `.ts/.tsx/.js/.jsx` file under `src/` for literal `t("...")`, `i18n.t("...")` / `i18next.t("...")` and `i18nKey="..."` keys and asserts each resolves to a string in `en.json` (the `fallbackLng`; i18next plural suffixes `_one`/`_other`/... accepted). Template-literal keys containing `${}` are dynamic: each must be listed in `DYNAMIC_KEY_PREFIXES` together with the en.json object prefix it expands under. The test fails on an unlisted dynamic template, on an allowlisted prefix that is missing or empty in en.json, and on stale allowlist entries. A sanity assertion (>500 literal usages found) stops the check from passing vacuously if the regex breaks. Both tests run in CI through the existing `pnpm test:unit` step of `.github/workflows/ci.yml` (Frontend Checks job), so no workflow change was needed.

**Files.** `tests/i18n/usedKeys.test.ts` (new), this ticket file.

**Commits.** `ae16292` (test), plus the follow-up commit recording these notes.

**Verification.**
- Pre-fix tree (locales from `939dcf3^`, temporarily written into `src/i18n/locales/` and restored from HEAD afterwards): `npx vitest run tests/i18n/usedKeys.test.ts tests/i18n/localeParity.test.ts` -> 4 failed / 11 passed. `usedKeys` lists the 10 missing keys (`claudeDesktop.route.stopBlockedByTakeover`, `codexConfig.noCommonConfigToApply`, `failover.tooltip.takeoverRequired`, `notifications.proxyReasonClaudeDesktop`, `provider.duplicateLiveIdsLoadFailed`, `providerForm.providerKeyStatusLoading`, `providerForm.requiredFields`, `proxy.server.stopFailed`, `proxy.server.stopped`, `usage.unpriced`); `localeParity` lists the 30 zh-TW missing `settings.*` keys and 2 zh-TW extra keys.
- Current tree (after CCS-021 and CCS-022): `npx vitest run tests/i18n` -> 3 files, 56 tests passed.
- `pnpm test:unit` -> 77 files, 523 tests passed. `pnpm typecheck` -> clean. `pnpm format:check` -> clean; `prettier --check tests/i18n/usedKeys.test.ts` -> clean.
- Out of scope: keys passed to `t()` via variables (e.g. `t(item.labelKey)`) cannot be checked statically and are not covered.
