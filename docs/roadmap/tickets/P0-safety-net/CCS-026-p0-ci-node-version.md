# CCS-026: [ci] Read the Node version from .node-version in CI and release

**Status:** Open · **Kind:** chore · **Labels:** `chore`, `phase:P0`, `p3-low`, `area:ci`

> **Ticket key:** `p0-ci-node-version` · **Phase:** P0 · **Priority:** p3-low · **Effort:** S
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Verified finding ci-node-version-mismatch. CI and release use Node "20" (EOL April 2026), while .node-version pins 22.12.0.

## Evidence
- ci.yml:22-24, release.yml:36-38 `node-version: 20`; .node-version 22.12.0

## Proposed change
Switch to `node-version-file: .node-version` and add `engines.node >=22.12.0` to package.json.

## Acceptance criteria / verification
- [ ] `grep -n node-version .github/workflows/*.yml` shows only node-version-file
- [ ] The CI log prints v22.12.0

## Dependencies
None.
