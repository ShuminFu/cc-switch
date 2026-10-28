# CCS-077: [security] Narrow the CSP connect-src and img-src

**Status:** Open · **Kind:** security · **Labels:** `security`, `phase:P1`, `p3-low`, `area:security`

> **Ticket key:** `p1-csp-tighten` · **Phase:** P1 · **Priority:** p3-low · **Effort:** S
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Verified finding ci-csp-too-wide, partially real. The critique notes that "tighten where feasible" had no exit criterion. The webview fetches only https://models.dev and loads https avatars and icons, but the CSP allows `connect-src https: http:` and `img-src http:`.

## Evidence
- src-tauri/tauri.conf.json:29; ModelsDevPickerDialog.tsx:27,164; CopilotAuthSection.tsx:417; ProviderIcon.tsx:85-88

## Proposed change
Set connect-src to `'self' ipc: http://ipc.localhost https://models.dev`, or move the fetch into a command. Remove `http:` from img-src.

## Acceptance criteria / verification
- [ ] `pnpm tauri dev`: the models.dev picker loads, the Copilot avatar and URL icons render, and there are no CSP violations
- [ ] In devtools, `fetch('https://example.com')` is blocked
- [ ] `grep -rn 'fetch(' src` confirms no other direct fetches

## Dependencies
None.
