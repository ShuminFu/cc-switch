# CCS-032: [ci] Document or automate promoting a prerelease to latest for the updater

**Status:** Open · **Kind:** chore · **Labels:** `chore`, `phase:P0`, `p3-low`, `area:ci`

> **Ticket key:** `p0-ci-prerelease-promotion` · **Phase:** P0 · **Priority:** p3-low · **Effort:** S
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Verified finding ci-prerelease-updater. Every release is created with prerelease: true, but the updater reads releases/latest, which skips prereleases. Promotion is a manual step that is not documented anywhere.

## Evidence
- release.yml:612; src-tauri/tauri.conf.json:65

## Proposed change
- Document `gh release edit vX.Y.Z --prerelease=false --latest` in docs/dev/RELEASING.md.
- Add a workflow_dispatch `promote-release` job.
- Optionally set prerelease from `contains(github.ref_name, '-')`.

## Acceptance criteria / verification
- [ ] RELEASING.md exists and is linked from CONTRIBUTING.md
- [ ] On a fork, after promotion, `curl -sIL .../releases/latest/download/latest.json` redirects to the new tag

## Dependencies
None.
