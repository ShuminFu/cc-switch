# CCS-107: [provider-form] Do not reset typed input when live settings finish loading in the edit dialog

**Status:** Open · **Kind:** bug · **Labels:** `bug`, `phase:P1`, `p3-low`, `area:frontend`

> **Ticket key:** `p1-fe-edit-dialog-reset` · **Phase:** P1 · **Priority:** p3-low · **Effort:** S
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Verified finding frontend-editdialog-reset-wipes-input, partially real. When editing the current provider (or an OpenClaw provider with a live entry), the form renders with DB settings. Once the live read resolves, form.reset wipes what the user typed and re-initialises the Codex and Gemini hooks.

## Evidence
- src/components/providers/EditProviderDialog.tsx:37, 45-132, 134, 162-179, 220-251; ProviderForm.tsx:379-401, 649-651; useCodexConfigState.ts:51; useGeminiConfigState.ts:77

## Proposed change
- Render ProviderForm only after hasLoadedLive, with a loading placeholder until then.
- Set hasLoadedLive on every path, including when live is falsy or the load is cancelled.

## Acceptance criteria / verification
- [ ] RTL: with a deferred live promise, the form appears only after it resolves and shows live values; a non-current provider renders immediately
- [ ] `pnpm test:unit`

## Dependencies
None.
