# CCS-031: [ci] Never cancel an in-progress release publish

**Status:** Open · **Kind:** chore · **Labels:** `chore`, `phase:P0`, `p3-low`, `area:ci`

> **Ticket key:** `p0-ci-release-concurrency` · **Phase:** P0 · **Priority:** p3-low · **Effort:** S
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Verified finding ci-release-cancel-in-progress. A workflow-level cancel-in-progress can cut publish-release or assemble-latest-json mid-upload. Cancelling during the build matrix is harmless.

## Evidence
- .github/workflows/release.yml:11-13

## Proposed change
- Use job-level concurrency: keep cancel-in-progress: true on the build matrix.
- Put publish-release and assemble-latest-json in a `release-publish-<ref>` group with cancel-in-progress: false.

## Acceptance criteria / verification
- [ ] actionlint passes
- [ ] Fork test: a re-pushed tag during publish waits rather than cancelling

## Dependencies
None.
