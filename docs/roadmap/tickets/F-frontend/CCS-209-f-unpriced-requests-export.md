# CCS-209: [usage] Filter and export unpriced requests, with a shortcut to add model pricing

**Status:** Open · **Kind:** feature · **Labels:** `feature`, `phase:F`, `p3-low`, `area:usage-ui`

> **Ticket key:** `f-unpriced-requests-export` · **Phase:** F · **Priority:** p3-low · **Effort:** S · **Depends on:** `p4-usage-repo`, `p0-i18n-missing-keys`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Frontend idea 10. Users cannot find which models are missing prices.

## Proposed change
- Add an "unpriced" filter on the request log and a CSV export grouped by model.
- Add an "Add pricing" action that pre-fills the model id.

## Acceptance criteria / verification
- [ ] RTL: the filter shows only unpriced rows; the export contains the model ids
- [ ] `pnpm test:unit`

## Dependencies
p4-usage-repo, p0-i18n-missing-keys
