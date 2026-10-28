# CCS-027: [ci] Add a minimal permissions block to ci.yml

**Status:** Open · **Kind:** security · **Labels:** `security`, `phase:P0`, `p3-low`, `area:ci`

> **Ticket key:** `p0-ci-permissions` · **Phase:** P0 · **Priority:** p3-low · **Effort:** S
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Verified finding ci-no-permissions-block. ci.yml uses the repo-default GITHUB_TOKEN scope, which may be write-all, on push to main and same-repo PRs.

## Evidence
- .github/workflows/ci.yml has no permissions key (the other workflows set one)

## Proposed change
Add a top-level `permissions: contents: read`.

## Acceptance criteria / verification
- [ ] `actionlint .github/workflows/ci.yml`
- [ ] The job log's GITHUB_TOKEN Permissions lists only Contents: read and Metadata: read

## Dependencies
None.
