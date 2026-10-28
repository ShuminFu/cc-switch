# CCS-192: [provider-form] Extract the OpenCode, OpenClaw and Hermes form modules, with component tests

**Status:** Open · **Kind:** refactor · **Labels:** `refactor`, `phase:F`, `p2-medium`, `area:frontend`

> **Ticket key:** `f-providerform-additive-apps` · **Phase:** F · **Priority:** p2-medium · **Effort:** L · **Depends on:** `f-providerform-shell`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Proposed change
Add per-app modules, plus component tests for HermesFormFields, OpenClawFormFields and OpenCodeFormFields.

## Acceptance criteria / verification
- [ ] Goldens unchanged; ProviderForm.tsx is 300 lines or fewer
- [ ] `pnpm typecheck` and `pnpm test:unit`

## Dependencies
f-providerform-shell
