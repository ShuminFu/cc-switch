# CCS-109: [skills] Dedupe the installed-skills cache by id and skip same-name folders in one ZIP

**Status:** Open · **Kind:** bug · **Labels:** `bug`, `phase:P1`, `p3-low`, `area:skills`

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
- [ ] Vitest: the cache has one entry for a duplicate id
- [ ] Rust test: a ZIP with a/foo and b/foo returns 1
- [ ] `pnpm test:unit` and `cargo test --manifest-path src-tauri/Cargo.toml install_from_zip`

## Dependencies
None.
