# CCS-042: [tooling] Make the icon extract and filter scripts runnable under type: module

**Status:** Open · **Kind:** bug · **Labels:** `bug`, `phase:P0`, `p3-low`, `area:tooling`

> **Ticket key:** `p0-icon-scripts-cjs` · **Phase:** P0 · **Priority:** p3-low · **Effort:** S
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Verified finding ci-icon-scripts-esm. Both scripts use require() and __dirname in an ESM package, so they throw a ReferenceError immediately.

## Evidence
- package.json:5 `"type": "module"`; scripts/extract-icons.js:1-2, scripts/filter-icons.js:1-2

## Proposed change
Rename both to .cjs and update the generated-by comment at extract-icons.js:201.

## Acceptance criteria / verification
- [ ] `node scripts/filter-icons.cjs` and `node scripts/extract-icons.cjs` run with no ReferenceError

## Dependencies
None.
