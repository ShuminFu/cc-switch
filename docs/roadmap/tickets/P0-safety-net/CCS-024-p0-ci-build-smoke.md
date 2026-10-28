# CCS-024: [ci] Add a vite production build and a Linux tauri build smoke to PR CI

**Status:** Done · **Kind:** chore · **Labels:** `chore`, `phase:P0`, `p2-medium`, `area:ci`

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
- [x] A PR importing a nonexistent CSS file fails CI
- [x] Locally: `pnpm build:renderer && pnpm tauri build --debug --no-bundle`

## Dependencies
None.

## Implementation notes

**Summary.** `.github/workflows/ci.yml`:
- `frontend` job gains a `Production build` step (`pnpm build:renderer`) after the unit tests. `tsc --noEmit` accepts any `*.css`/asset side-effect import (vite client ambient types), so only a real vite/rollup build catches an unresolved import.
- New `tauri-build-smoke` job on `ubuntu-22.04` (Node 20 + pnpm 10.12.3, toolchain from `rust-toolchain.toml`, same apt deps as `backend`, its own `cargo-tauri-smoke` cache key) runs `pnpm tauri build --debug --no-bundle`. `beforeBuildCommand` runs `build:renderer`, so the binary embeds the real `dist` rather than the `mkdir -p dist` placeholder; a final step asserts `dist/index.html` and `src-tauri/target/debug/cc-switch` exist. `--no-bundle` skips packaging and updater signing, so no secrets are required (works for fork PRs).
- Because `release.yml` calls `ci.yml` via `workflow_call` (CCS-023), both new checks also gate release builds.

**Files.** `.github/workflows/ci.yml`, this ticket file.

**Commits.** See `git log --grep CCS-024` (single commit containing the workflow change and these notes).

**Verification.**
- `pnpm build:renderer` -> exit 0 (built in ~19 s).
- Negative check: temporarily prepended `import "./does-not-exist.css";` to `src/main.tsx` -> `pnpm build:renderer` exit 1 (`Could not resolve "./does-not-exist.css" from "src/main.tsx"`), while `pnpm typecheck` with the same import exits 0, proving the new step is what catches it. File restored afterwards.
- `pnpm tauri build --debug --no-bundle` -> exit 0, `Built application at: src-tauri/target/debug/cc-switch` (~1m54s with a warm cache).
- `python3 -c "import yaml,sys;yaml.safe_load(open(sys.argv[1]))" .github/workflows/ci.yml` -> OK; `actionlint .github/workflows/ci.yml` -> exit 0.
- Not verifiable locally: the job's actual run on GitHub-hosted runners (first PR after merge). Making `Tauri Build Smoke (Linux)` a required status check is a repository-settings change for a maintainer.
