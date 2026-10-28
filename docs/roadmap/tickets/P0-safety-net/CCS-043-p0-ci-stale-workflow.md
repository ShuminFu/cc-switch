# CCS-043: [ci] Remove the invalid stale.yml input and implement a real popular-issue exemption

**Status:** Open · **Kind:** bug · **Labels:** `bug`, `phase:P0`, `p3-low`, `area:ci`

> **Ticket key:** `p0-ci-stale-workflow` · **Phase:** P0 · **Priority:** p3-low · **Effort:** S
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Verified finding ci-stale-invalid-input. `exempt-issue-created-after` is not an actions/stale input, so the "3+ reactions" exemption never applies and popular issues get closed.

## Evidence
- .github/workflows/stale.yml:40-41

## Proposed change
- Remove the line and its comment.
- Add a `popular` label to exempt-issue-labels, applied by a small github-script step to issues with 3 or more reactions.

## Acceptance criteria / verification
- [ ] `actionlint` passes; a workflow_dispatch run with debug-only: true has no Unexpected input warning and skips labelled issues

## Dependencies
None.
