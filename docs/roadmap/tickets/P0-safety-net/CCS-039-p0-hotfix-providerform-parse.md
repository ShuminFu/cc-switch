# CCS-039: [provider-form] Block save when Codex or Gemini config fails to parse instead of silently saving the initial config

**Status:** Open · **Kind:** bug · **Labels:** `bug`, `phase:P0`, `p0-critical`, `area:frontend`

> **Ticket key:** `p0-hotfix-providerform-parse` · **Phase:** P0 · **Priority:** p0-critical · **Effort:** S
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Verified finding frontend-providerform-parse-fallback.
- When JSON.parse(codexAuth) fails (including an empty auth.json, which shows no error), Codex save falls back to values.settingsConfig. That is the initial or preset value, so every Codex edit is lost, and a success toast still appears.
- Gemini behaves the same with invalid config JSON, except for quick-field edits.

## Evidence
- src/components/providers/forms/ProviderForm.tsx:1284-1323 (Codex catch), 1325-1336 (Gemini), handleSubmit 998-1256 ignores codexAuthError/codexConfigError/geminiConfigError
- useCodexConfigState.ts:172-174 treats "" as valid

## Proposed change
- In handleSubmit, hard-reject with toast.error when the Codex auth is non-empty and invalid, or codexConfigError is set, and likewise for Gemini.
- Parse an empty auth as {}.
- Remove the values.settingsConfig fallbacks and narrow the try scope.

## Acceptance criteria / verification
- [ ] RTL test: Codex with config.toml edited to base_url B and auth "{bad" means onSubmit is not called and an error toast appears; with auth "", onSubmit gets auth {} and base_url B
- [ ] Gemini invalid config blocks submit
- [ ] `pnpm typecheck` and `pnpm test:unit`

## Dependencies
None.
