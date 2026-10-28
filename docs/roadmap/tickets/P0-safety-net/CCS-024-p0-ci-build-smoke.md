# CCS-024: [ci] Add a vite production build and a Linux tauri build smoke to PR CI

**Status:** Open · **Kind:** chore · **Labels:** `chore`, `phase:P0`, `p2-medium`, `area:ci`

> **Ticket key:** `p0-ci-build-smoke` · **Phase:** P0 · **Priority:** p2-medium · **Effort:** S
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Verified finding ci-no-build-smoke. PR CI uses an empty placeholder dist. Vite/rollup and tauri bundle failures show up only when a release tag is pushed.

## Evidence
- .github/workflows/ci.yml backend job `mkdir -p dist`; package.json:11 build:renderer never run in CI

## Proposed change
- Add `pnpm build:renderer` to the frontend job.
- Add an ubuntu-22.04 step that runs `pnpm tauri build --debug --no-bundle` against the real dist.

## Acceptance criteria / verification
- [ ] A PR importing a nonexistent CSS file fails CI
- [ ] Locally: `pnpm build:renderer && pnpm tauri build --debug --no-bundle`

## Dependencies
None.
