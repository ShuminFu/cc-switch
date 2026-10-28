# CCS-115: [errors] Switch frontend error handling on IpcError.code

**Status:** Open · **Kind:** refactor · **Labels:** `refactor`, `phase:P2`, `p2-medium`, `area:frontend`

> **Ticket key:** `p2-ipc-error-frontend` · **Phase:** P2 · **Priority:** p2-medium · **Effort:** S · **Depends on:** `p2-ipc-error-type`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
errorUtils and skillErrorParser match localized message substrings, so any rewording breaks them.

## Evidence
- src/utils/errorUtils.ts:53-64; skillErrorParser.ts

## Proposed change
Parse IpcError and switch on code. Keep a legacy string fallback until 100% of commands are migrated.

## Acceptance criteria / verification
- [ ] `grep -nP '[\x{4e00}-\x{9fff}]' src/utils/errorUtils.ts` finds no matching code
- [ ] Vitest per code
- [ ] `pnpm typecheck` and `pnpm test:unit`

## Dependencies
p2-ipc-error-type
