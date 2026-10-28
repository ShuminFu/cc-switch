# CCS-190: [provider-form] Turn ProviderForm into a shell with a base schema, a discriminated union and an inline error summary

**Status:** Open · **Kind:** refactor · **Labels:** `refactor`, `phase:F`, `p2-medium`, `area:frontend`

> **Ticket key:** `f-providerform-shell` · **Phase:** F · **Priority:** p2-medium · **Effort:** L · **Depends on:** `f-settingsconfig-goldens`, `p2-fe-app-registry`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Evidence
- src/components/providers/forms/ProviderForm.tsx (2578 lines)

## Proposed change
- Add features/providers/forms/ with a base schema plus a per-app discriminated union, and Fields components using useFormContext.
- Add a shared BasePreset.
- Add an inline error summary that blocks Save across all editors (frontend idea 1).

## Acceptance criteria / verification
- [ ] The shell is 300 lines or fewer; the settingsConfig goldens are unchanged
- [ ] `pnpm typecheck` and `pnpm test:unit`

## Dependencies
f-settingsconfig-goldens, p2-fe-app-registry
