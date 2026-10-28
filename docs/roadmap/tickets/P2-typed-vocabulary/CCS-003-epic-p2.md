# CCS-003: [epic] P2 (v3.20): Typed vocabulary, app registry, IPC contract and generated bindings

**Status:** Open · **Kind:** epic · **Labels:** `epic`, `phase:P2`, `p1-high`, `area:architecture`

> **Ticket key:** `epic-p2` · **Phase:** P2 · **Priority:** p1-high · **Effort:** XL · **Depends on:** `epic-p1`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Goal
Add the shared types that every later move depends on, alongside the existing code, so the compiler checks dispatch before any code moves. Bug-report capture moves to P3 (critique), because CaptureTap needs the P3 stream-tap layer.

## Exit criteria
- [ ] openai_responses/openai_chat/gemini_native string literals appear only in format.rs parse/serialize (10 or fewer)
- [ ] #[allow(dead_code)] in proxy/ goes from 69 (actual count, per the critique) to 15 or fewer
- [ ] No hard-coded app list at lib.rs:1754, services/proxy.rs:1308, tray.rs:157 or App.tsx:124; the registry exhaustiveness test passes
- [ ] At least 50% of commands return IpcError; errorUtils.ts has no Chinese-substring matching
- [ ] bindings.ts is generated and dirty-checked in CI (or the ts-rs fallback decision is recorded); raw invoke() outside the bindings/api layer is an ESLint error
- [ ] Frontend app-literal comparisons are at 200 or fewer

## Child tickets
p2-proxy-format-types, p2-proxy-dead-code-purge, p2-app-registry, p2-app-registry-ipc, p2-ipc-error-type, p2-ipc-error-frontend, p2-bindings-spike, p2-bindings-generate, p2-ipc-naming-orphans, p2-fe-app-registry, p2-eslint-ratchets, p2-deps-modernize, p2-feature-actionable-errors
