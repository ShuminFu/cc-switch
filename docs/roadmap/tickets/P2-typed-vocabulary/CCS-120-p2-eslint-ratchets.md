# CCS-120: [lint] Ban raw invoke outside the bindings layer and ratchet app-literal comparisons

**Status:** Open · **Kind:** chore · **Labels:** `chore`, `phase:P2`, `p2-medium`, `area:frontend`

> **Ticket key:** `p2-eslint-ratchets` · **Phase:** P2 · **Priority:** p2-medium · **Effort:** S · **Depends on:** `p2-bindings-generate`, `p2-fe-app-registry`, `p0-ci-eslint-prettier`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Nothing stops new code from bypassing the bindings or adding app-id literal comparisons (283 today).

## Proposed change
- Make no-restricted-imports for `invoke` an error outside src/shared/bindings.ts and src/lib/api.
- Add a custom rule or script that counts app-literal comparisons, with a baseline file.

## Acceptance criteria / verification
- [ ] A new raw invoke fails `pnpm lint`
- [ ] The literal count is 200 or fewer and enforced

## Dependencies
p2-bindings-generate, p2-fe-app-registry, p0-ci-eslint-prettier
