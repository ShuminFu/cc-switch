# CCS-109: [skills] Dedupe the installed-skills cache by id and skip same-name folders in one ZIP

**Status:** Done · **Kind:** bug · **Labels:** `bug`, `phase:P1`, `p3-low`, `area:skills`

> **Ticket key:** `p1-fe-skills-cache-dedupe` · **Phase:** P1 · **Priority:** p3-low · **Effort:** S
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Verified finding frontend-skills-duplicate-cache, partially real. The only real trigger is one ZIP holding two folders with the same install name. Both are saved as local:<name> and both are appended to the cache, producing duplicate React keys.

## Evidence
- src/hooks/useSkills.ts:82-88, 282-288; useSkills.helpers.ts:10-19 (mergeImportedSkills); src-tauri/src/services/skill.rs:2552, 2602-2615

## Proposed change
- Use mergeImportedSkills in both install hooks, last entry per id wins.
- In install_from_zip, keep a HashSet of install names inside the loop.

## Acceptance criteria / verification
- [x] Vitest: the cache has one entry for a duplicate id
- [x] Rust test: a ZIP with a/foo and b/foo returns 1
- [x] `pnpm test:unit` and `cargo test --manifest-path src-tauri/Cargo.toml install_from_zip`

## Dependencies
None.

## Implementation notes

**Summary.** `SkillService::install_from_zip` keeps a `HashSet` of install names (case-insensitive) seen in the current archive and skips any later folder with the same install name, logging a warning; a ZIP with `a/foo` and `b/foo` now installs exactly one `local:foo`. On the frontend, `mergeImportedSkills` now also dedupes ids within the incoming result (last entry per id wins), and both `useInstallSkill` and `useInstallSkillsFromZip` update the `["skills","installed"]` cache through it instead of appending blindly.

**Files.**
- `src-tauri/src/services/skill.rs` (install_from_zip dedupe)
- `src-tauri/tests/skill_sync.rs` (new test `install_from_zip_skips_duplicate_install_names_in_one_archive`)
- `src/hooks/useSkills.helpers.ts`, `src/hooks/useSkills.ts`
- `tests/hooks/useImportSkillsFromApps.test.tsx` (3 new cases for duplicate ids inside one result / single-install replacement)

**Commits.** `b554574` (code + tests); this ticket update in a follow-up docs commit.

**Verification.**
- `cargo test --manifest-path src-tauri/Cargo.toml --test skill_sync install_from_zip`: 1 passed; with the skill.rs change reverted it fails (`left: 2, right: 1`).
- `cargo test --manifest-path src-tauri/Cargo.toml --no-fail-fast`: 2464 passed, 0 failed.
- `cargo fmt` clean; `cargo clippy -- -D warnings` clean.
- `pnpm typecheck`: pass. `pnpm test:unit`: 147 files / 1448 tests passed. `prettier --check` on touched files: clean.
