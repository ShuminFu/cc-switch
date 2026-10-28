# CCS-071: [deeplink] Do not replace the request under an open confirm dialog

**Status:** Open · **Kind:** security · **Labels:** `security`, `phase:P1`, `p3-low`, `area:deeplink`

> **Ticket key:** `p1-deeplink-dialog-guard` · **Phase:** P1 · **Priority:** p3-low · **Effort:** S
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Verified finding frontend-deeplink-request-replaced, partially real. A new deeplink-import event swaps `request` while the dialog is open, so the user can confirm content they never reviewed. The async merge race is not realistic today.

## Evidence
- src/components/DeepLinkImportDialog.tsx:53-77, 94-100; src-tauri/src/lib.rs:154

## Proposed change
- Ignore or queue incoming requests while isOpen or isImporting (use refs), and show a toast.
- Add a sequence number for when remote configUrl fetching arrives.

## Acceptance criteria / verification
- [ ] Vitest with a mocked `listen`: emit A, then B; the dialog still shows A; Import calls importFromDeeplink with A
- [ ] `pnpm test:unit`

## Dependencies
None.
