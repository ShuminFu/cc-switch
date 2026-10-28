# CCS-039: [provider-form] Block save when Codex or Gemini config fails to parse instead of silently saving the initial config

**Status:** Done · **Kind:** bug · **Labels:** `bug`, `phase:P0`, `p0-critical`, `area:frontend`

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
- [x] RTL test: Codex with config.toml edited to base_url B and auth "{bad" means onSubmit is not called and an error toast appears; with auth "", onSubmit gets auth {} and base_url B
- [x] Gemini invalid config blocks submit
- [x] `pnpm typecheck` and `pnpm test:unit`

## Dependencies
None.

## Implementation notes
- `ProviderForm.tsx`: added the exported helper `parseJsonObjectForSave`. It parses empty text as `{}` and returns `null` for invalid JSON or for a value that is not an object.
- `handleSubmit` now blocks the save with `toast.error` (B-class hard reject, before the soft-issue dialog) in three cases: Codex auth is non-empty and invalid; Codex `config.toml` fails a synchronous TOML re-validation (`validateToml`; the debounced `codexConfigError` can be stale at submit time); Gemini config JSON is invalid.
- `performSubmit`: removed both `values.settingsConfig` fallbacks and the broad `try/catch`. Only the JSON parse is guarded, and a parse failure now aborts instead of saving the initial or preset config.
- i18n: added `providerForm.codexAuthInvalid`, `providerForm.codexConfigInvalid` and `providerForm.geminiConfigInvalid` in en/zh/zh-TW/ja.
- Tests: `tests/components/ProviderForm.parseGuard.test.tsx` renders the real form (only JsonEditor and sonner are mocked). It covers four cases: Codex auth `{bad` with config base_url B means no submit and an error toast; Codex auth `""` submits auth `{}` and base_url B; invalid Codex TOML is blocked; invalid Gemini config is blocked. All 4 tests fail against the pre-change ProviderForm.tsx and pass after.
- Files: `src/components/providers/forms/ProviderForm.tsx`, `src/i18n/locales/{en,zh,zh-TW,ja}.json`, `tests/components/ProviderForm.parseGuard.test.tsx`.
- Commit: 5ce06b0 (implementation + tests); SHA recorded in a follow-up docs commit.
- Verification: `pnpm typecheck` passed; `pnpm test:unit` passed (74 files, 467 tests); `pnpm format:check` passed.
- Review: re-checked every acceptance criterion against 5ce06b0. The hard-reject guards run before the soft-issue dialog, `performSubmit` no longer falls back to `values.settingsConfig` for Codex or Gemini (the only remaining fallback is the generic branch for other apps, which is out of scope), and the synchronous `validateToml` re-check closes the stale debounce window. No other call site builds Codex or Gemini settings from the editor text. Follow-up: the parse-guard tests previously accepted any `toast.error` call; they now assert the specific auth.json, config.toml and Gemini error messages, so an unrelated error toast cannot satisfy them. Re-verified with `pnpm typecheck`, `pnpm test:unit` (74 files, 467 tests) and `pnpm format:check`.
