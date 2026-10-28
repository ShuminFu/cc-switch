# CCS-200: [ui] Move custom overlays to Radix Dialog and add component tests for the large panels

**Status:** Open · **Kind:** refactor · **Labels:** `refactor`, `phase:F`, `p3-low`, `area:frontend`

> **Ticket key:** `f-radix-dialog-component-tests` · **Phase:** F · **Priority:** p3-low · **Effort:** M · **Depends on:** `f-providerform-core-apps`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Proposed change
- Replace the custom `fixed inset-0` overlays with Radix Dialog (focus trap, Esc).
- Add component tests for CodexFormFields, ProxyPanel and UsageScriptModal.

## Acceptance criteria / verification
- [ ] `grep -rn 'fixed inset-0' src/components` matches only Radix internals
- [ ] The new tests pass under `pnpm test:unit`

## Dependencies
f-providerform-core-apps
