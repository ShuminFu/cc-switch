# CCS-206: [provider-form] Guard against closing the Add/Edit panel with unsaved changes

**Status:** Open · **Kind:** feature · **Labels:** `feature`, `phase:F`, `p3-low`, `area:frontend`

> **Ticket key:** `f-unsaved-changes-guard` · **Phase:** F · **Priority:** p3-low · **Effort:** S · **Depends on:** `f-providerform-shell`, `f-radix-dialog-component-tests`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Frontend idea 3. Pressing Esc or clicking outside discards edits silently.

## Proposed change
When formState.isDirty or an editor is dirty, intercept close and show a confirm dialog.

## Acceptance criteria / verification
- [ ] RTL: edit, press Esc, the confirm appears; cancel keeps the edits
- [ ] `pnpm test:unit`

## Dependencies
f-providerform-shell, f-radix-dialog-component-tests
