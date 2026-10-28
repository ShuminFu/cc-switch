# CCS-072: [deeplink] Use one masking helper that shows at most 4 characters

**Status:** Open · **Kind:** security · **Labels:** `security`, `phase:P1`, `p3-low`, `area:deeplink`

> **Ticket key:** `p1-deeplink-mask-value` · **Phase:** P1 · **Priority:** p3-low · **Effort:** S
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Verified finding frontend-deeplink-masking-weak. maskValue shows 8 characters, and shows sensitive values of 8 characters or fewer in full. The apiKey mask shows 4.

## Evidence
- src/components/DeepLinkImportDialog.tsx:281-290 vs 209-213; used at 538, 565, 601

## Proposed change
One shared helper: the first 4 characters plus fixed asterisks, or "****" for values of 4 characters or fewer.

## Acceptance criteria / verification
- [ ] Vitest: maskValue('ANTHROPIC_AUTH_TOKEN','abcdefgh') == '****'; 'sk-abcdefghijk' gives 'sk-a' plus asterisks; non-sensitive keys are unchanged
- [ ] `pnpm typecheck` and `pnpm test:unit`

## Dependencies
None.
