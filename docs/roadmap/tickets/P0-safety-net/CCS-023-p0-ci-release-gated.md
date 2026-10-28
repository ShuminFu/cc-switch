# CCS-023: [ci] Require green CI on the tag SHA before release builds

**Status:** Open · **Kind:** chore · **Labels:** `chore`, `phase:P0`, `p1-high`, `area:ci`

> **Ticket key:** `p0-ci-release-gated` · **Phase:** P0 · **Priority:** p1-high · **Effort:** S
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Verified finding ci-release-not-gated-on-ci. release.yml runs on a v* tag push without CI and runs no tests, so a broken commit can publish a prerelease.

## Evidence
- .github/workflows/release.yml:3-6 trigger, :16 release job without needs; ci.yml:3-7 only runs on PRs and main

## Proposed change
Add `workflow_call:` to ci.yml. In release.yml, add a `ci` job (`uses: ./.github/workflows/ci.yml`) and set `needs: ci` on `release`. Add required reviewers on the `release` environment.

## Acceptance criteria / verification
- [ ] `actionlint` passes
- [ ] On a fork, a test tag on a commit with a failing test skips release and publish-release

## Dependencies
None.
