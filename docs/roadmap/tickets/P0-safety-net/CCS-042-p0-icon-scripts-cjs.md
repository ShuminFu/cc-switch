# CCS-042: [tooling] Make the icon extract and filter scripts runnable under type: module

**Status:** Done · **Kind:** bug · **Labels:** `bug`, `phase:P0`, `p3-low`, `area:tooling`

> **Ticket key:** `p0-icon-scripts-cjs` · **Phase:** P0 · **Priority:** p3-low · **Effort:** S
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Verified finding ci-icon-scripts-esm. Both scripts use require() and __dirname in an ESM package, so they throw a ReferenceError immediately.

## Evidence
- package.json:5 `"type": "module"`; scripts/extract-icons.js:1-2, scripts/filter-icons.js:1-2

## Proposed change
Rename both to .cjs and update the generated-by comment at extract-icons.js:201.

## Acceptance criteria / verification
- [x] `node scripts/filter-icons.cjs` and `node scripts/extract-icons.cjs` run with no ReferenceError

## Dependencies
None.

## Implementation notes

**Summary.** `scripts/extract-icons.js` and `scripts/filter-icons.js` were
renamed to `.cjs`, so Node loads them as CommonJS despite `"type": "module"`
in `package.json`. The generated-by line in the README template
(extract-icons, ~line 201) now says `scripts/extract-icons.cjs`.
`filter-icons` also ended with `require('./generate-icon-index.js')`, a file
that was never committed. Once the ReferenceError was fixed, the script would
have crashed with MODULE_NOT_FOUND after it had already deleted and renamed
the icons. It now loads `generate-icon-index.cjs` only when that file exists.
Otherwise it prints a note that `src/icons/extracted/index.ts` (maintained
by hand) has to be updated manually. Nothing references the old `.js` names
(no package.json scripts, docs or CI).

**Files.**
- `scripts/extract-icons.cjs` (renamed from `.js`, generated-by line)
- `scripts/filter-icons.cjs` (renamed from `.js`, guarded generator require)
- `tests/scripts/iconScripts.test.ts` (new regression test: it copies both
  scripts into a temp tree with `{"type":"module"}`, runs them with node and
  asserts exit 0, no ReferenceError and the expected file effects. It also
  checks that the `.js` names are gone.)

**Commits.** f1eabaa (code + test), plus the follow-up commit that records
these notes.

**Verification.**
- Before: the HEAD `.js` scripts under a `"type": "module"` package.json both
  fail with `ReferenceError: require is not defined in ES module scope`.
- After: `node scripts/filter-icons.cjs` and `node scripts/extract-icons.cjs`,
  run against a scratch copy of `src/icons/extracted` plus the real
  `node_modules` (so the repo's icons were not touched), both exit 0 with no
  ReferenceError. extract reported 8 of its list not found in the
  `@lobehub` package; this is expected and is the existing behaviour.
- `pnpm vitest run tests/scripts/iconScripts.test.ts`: 3 passed.
- `pnpm typecheck`: clean. `pnpm test:unit`: 90 files / 641 tests passed.
  `pnpm format:check`: all files pass.
