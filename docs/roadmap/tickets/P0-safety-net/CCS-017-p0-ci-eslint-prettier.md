# CCS-017: [ci] Add ESLint with a baseline and extend Prettier to tests/

**Status:** Open · **Kind:** chore · **Labels:** `chore`, `phase:P0`, `p2-medium`, `area:ci`

> **Ticket key:** `p0-ci-eslint-prettier` · **Phase:** P0 · **Priority:** p2-medium · **Effort:** M
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Verified finding ci-no-eslint-tests-format: no frontend linter exists, and format:check covers only src/.

## Evidence
- package.json:14 `prettier --check "src/**/*..."`; ci.yml frontend job has no lint step

## Proposed change
- Add eslint.config.js (typescript-eslint, react-hooks, @tanstack/eslint-plugin-query) at warn level, with a committed baseline that fails only on new warnings.
- Add a `lint` script and a CI step.
- Change the format globs to `{src,tests}/**/*`, after running `pnpm format` once.

## Acceptance criteria / verification
- [ ] `pnpm lint` and `pnpm format:check` pass
- [ ] A rules-of-hooks violation in a new file fails CI
- [ ] `pnpm typecheck` and `pnpm test:unit` unaffected

## Dependencies
None.
