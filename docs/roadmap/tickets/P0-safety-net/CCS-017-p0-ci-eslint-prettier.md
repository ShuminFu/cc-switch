# CCS-017: [ci] Add ESLint with a baseline and extend Prettier to tests/

**Status:** Done · **Kind:** chore · **Labels:** `chore`, `phase:P0`, `p2-medium`, `area:ci`

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
- [x] `pnpm lint` and `pnpm format:check` pass
- [x] A rules-of-hooks violation in a new file fails CI
- [x] `pnpm typecheck` and `pnpm test:unit` unaffected

## Dependencies
None.

## Implementation notes

**Summary.** Added a flat `eslint.config.js` (ESLint 9, `@eslint/js` recommended,
`typescript-eslint` recommended, `@tanstack/eslint-plugin-query` flat/recommended,
`react-hooks/rules-of-hooks` + `react-hooks/exhaustive-deps`) covering `src/**/*.{ts,tsx}`
and `tests/**/*.{ts,tsx}`. The baseline is ESLint's native bulk-suppression file
`eslint-suppressions.json` (261 pre-existing problems across 58 files, generated with
`eslint . --suppress-all`; it contains no `rules-of-hooks` entries). `pnpm lint`
(`eslint . --max-warnings 0`) fails on any new violation, including a file exceeding its
per-rule baseline count; `pnpm lint:prune` shrinks the baseline after fixes.

Deviation from "warn level": ESLint bulk suppressions only apply to rules at `"error"`,
so the rules are configured as errors (the one recommended `warn` rule,
`@tanstack/query/no-rest-destructuring`, is promoted) and `--max-warnings 0` keeps any
future warn-level rule from slipping through. The effect is the one the ticket asks for:
existing problems are tolerated, new ones fail CI.

`format`/`format:check` now glob `{src,tests}/**/*`; `pnpm format` was run once (9 test
files reformatted, whitespace only). CI frontend job gained a `Lint` step before
`Check formatting`. `@typescript-eslint/no-unused-vars` is off because `tsc`
(`noUnusedLocals`/`noUnusedParameters`) already enforces it.

**Files.** `eslint.config.js`, `eslint-suppressions.json`, `package.json`, `pnpm-lock.yaml`
(devDeps: eslint 9.39.5, @eslint/js, typescript-eslint 8.72.1, eslint-plugin-react-hooks
7.1.1, @tanstack/eslint-plugin-query 5.105.0, globals), `.github/workflows/ci.yml`,
`tests/lint/eslintConfig.test.ts` (new), 9 reformatted files under `tests/`.

**Commits.** `447e745` (implementation), plus the follow-up commit recording these notes.

**Verification.**
- `pnpm lint` -> exit 0; `pnpm format:check` -> "All matched files use Prettier code style!"
- New file `src/__tmp/Bad.tsx` with a conditional `useState` -> `pnpm lint` exit 1
  (`react-hooks/rules-of-hooks` error); adding one extra `any` to `src/App.tsx` -> exit 1
  (suppression count exceeded). Both scratch changes reverted.
- `tests/lint/eslintConfig.test.ts` (4 tests): passes; with `eslint.config.js` removed,
  3 of 4 fail (regression check).
- `pnpm typecheck` -> OK; `pnpm test:unit` -> 78 files / 527 tests passed (one earlier run
  had 2 transient failures under CPU contention from a concurrent cargo build; two
  subsequent full runs were green).
- `ci.yml` parses with `yaml.safe_load`; actionlint not installed locally.
- `pnpm@10.12.3 install --frozen-lockfile` (CI's pnpm version) accepts the lockfile.
