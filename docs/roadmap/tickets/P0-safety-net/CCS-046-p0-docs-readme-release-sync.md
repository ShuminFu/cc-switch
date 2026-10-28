# CCS-046: [docs] Update the README release sections: release-notes links and Linux asset names

**Status:** Done · **Kind:** chore · **Labels:** `chore`, `phase:P0`, `p3-low`, `area:docs`

> **Ticket key:** `p0-docs-readme-release-sync` · **Phase:** P0 · **Priority:** p3-low · **Effort:** S
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Covers verified findings ci-readme-stale-links and ci-readme-linux-asset-names. Both are fixed by one edit to the release and installation sections of the four READMEs.
- The READMEs link v3.16.1 release notes, although v3.17.0 notes exist.
- They list Linux assets without the arch suffix and omit arm64.

## Evidence
- README.md:218, README_DE.md:218, README_JA.md:218, README_ZH.md:219 (links)
- README.md/DE/JA:388-390, README_ZH.md:391-393 (assets) vs release.yml:515-547,625-626

## Proposed change
Point the links at v3.17.0-{en,ja,zh}.md (DE uses en). List CC-Switch-v{version}-Linux-{x86_64|arm64}.{deb,rpm,AppImage}.

## Acceptance criteria / verification
- [x] `grep -n 'release-notes/v' README*.md` shows only v3.17.0, and each linked file exists
- [x] `grep -n 'Linux\.\(deb\|rpm\|AppImage\)' README*.md` finds nothing

## Dependencies
None (p0-ci-release-hygiene's version check can later guard this).

## Implementation notes
- **Summary:** README.md, README_DE.md, README_JA.md and README_ZH.md now link `docs/release-notes/v3.17.0-{en,ja,zh}.md` (DE uses `en`). The Linux install section lists `CC-Switch-v{version}-Linux-{x86_64|arm64}.{deb,rpm,AppImage}`. This matches the `CC-Switch-${VERSION}-Linux-${ARCH}` names that the "Prepare Linux Assets" step in `.github/workflows/release.yml` publishes, where `ARCH` is `x86_64` or `arm64`.
- **Files:** `README.md`, `README_DE.md`, `README_JA.md`, `README_ZH.md`.
- **Commits:** `04b7626` (README edits), plus the follow-up commit that records these notes.
- **Verification:**
  - `grep -n 'release-notes/v' README*.md` returns only v3.17.0 links (README.md:218, README_DE.md:218, README_JA.md:218, README_ZH.md:219). `test -f` passes for each linked file (`v3.17.0-en.md`, `v3.17.0-ja.md`, `v3.17.0-zh.md`).
  - `grep -n 'Linux\.\(deb\|rpm\|AppImage\)' README*.md` returns nothing (exit 1).
  - Docs-only change, so no automated test applies. The READMEs are outside the `pnpm format:check` globs. The prettier warning on README_DE.md was already there before this change; it shows up on the HEAD version too.
