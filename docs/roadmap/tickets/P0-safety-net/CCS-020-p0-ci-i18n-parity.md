# CCS-020: [i18n] Add a locale parity and used-key check to CI

**Status:** Open · **Kind:** chore · **Labels:** `chore`, `phase:P0`, `p2-medium`, `area:i18n`

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
- [ ] The test fails on the current tree, listing the 10 missing and 30 zh-TW keys, and passes after the two key-fill tickets
- [ ] `pnpm test:unit`

## Dependencies
None (lands together with p0-i18n-missing-keys and p0-i18n-zhtw-keys).
