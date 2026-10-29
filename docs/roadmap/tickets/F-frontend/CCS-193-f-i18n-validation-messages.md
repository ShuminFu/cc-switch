# CCS-193: [i18n] Localize zod and editor validation messages

**Status:** Partial · **Kind:** bug · **Labels:** `bug`, `phase:F`, `p3-low`, `area:i18n`

> **Ticket key:** `f-i18n-validation-messages` · **Phase:** F · **Priority:** p3-low · **Effort:** M · **Depends on:** `f-providerform-shell`, `p0-ci-i18n-parity`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Verified finding frontend-hardcoded-strings, partially real. Provider Zod messages and parseJsonError are Chinese. validateJsonConfig builds Chinese messages. useCodexConfigState and useManagedAuth use English-only literals. mcp.ts is dead code.

## Evidence
- src/lib/schemas/provider.ts:6-48; src/utils/providerConfigUtils.ts:73-89; useCommonConfigSnippet.ts:261,340; useCodexConfigState.ts:177,181; useManagedAuth.ts:84,120; src/lib/schemas/mcp.ts (unused)

## Proposed change
- Add createProviderSchema(t), and have parseJsonError and validateJsonConfig take t or return codes.
- Add the keys to all 4 locales and delete mcp.ts.

## Acceptance criteria / verification
- [ ] Vitest with en: createProviderSchema(t).safeParse({name:'x',settingsConfig:'{bad'}) contains no CJK (/[一-鿿]/)
- [ ] `grep -rnP '[\x{4e00}-\x{9fff}]' src/lib src/utils src/components/providers/forms/hooks --include=*.ts` matches only comments
- [ ] `pnpm typecheck` and `pnpm test:unit`

## Dependencies
f-providerform-shell, p0-ci-i18n-parity

## Implementation notes

**Summary**
- `src/lib/schemas/provider.ts`: new `createProviderSchema(t)`; URL/required messages use zod 4 lazy `error: () => t(...)`, so they resolve at parse time. `parseJsonError(error, t)` is exported and builds messages from `providerForm.validation.jsonSyntax*` keys (the old ad-hoc Chinese token rewriting is gone; the engine detail is kept). `providerSchema` = `createProviderSchema(i18next.t)` so the static discriminated union (`src/features/providers/forms/schema.ts`) follows the current language without changes.
- `src/utils/providerConfigUtils.ts`: `validateJsonConfig(value, t, field)` (field is a typed `JsonConfigField` code: `config` / `commonConfigSnippet` / `extractedConfig`) and `updateCommonConfigSnippet(..., enabled, t)`; callers in `useCommonConfigSnippet.ts` pass `t` (and list it in hook deps).
- `useCodexConfigState.ts` (auth.json errors), `useCodexTomlValidation.ts` (fallback message) and `useManagedAuth.ts` (device code expired) now use `t`.
- `src/utils/tomlUtils.ts` (MCP TOML conversion errors) and `src/lib/schemas/settings.ts` (path required, lazy) use `i18next.t`.
- Developer console logs in the scanned areas (`useCommonConfigSnippet`, `useCodexCommonConfig`, `useGeminiCommonConfig`, `useTemplateValues`, `lib/api/env.ts`, `lib/query/queries.ts`) translated to English.
- Dead `src/lib/schemas/mcp.ts` and `src/lib/schemas/common.ts` (no importers) deleted. Note: these staged deletions were swept into the concurrent Rust-lane commit `ed7aac2` (CCS-079) because it committed while they were in the index.
- New keys in all 4 locales: `providerForm.validation.*` (15), `managedAuth.deviceCodeExpired`, `mcp.error.{tomlEmpty,tomlUnrecognized,serverConfigNotObject,stdioCommandRequired,urlFieldRequired,unsupportedType}`, `settings.validation.pathRequired`.

**Tests**
- New `tests/lib/validationMessagesI18n.test.ts`: with an `en` i18next instance, `createProviderSchema(t).safeParse({name:'x',settingsConfig:'{bad'})` has no CJK; URL/required/`parseJsonError`/`validateJsonConfig`/`updateCommonConfigSnippet` messages match en.json; zh instance still yields Chinese; static `providerSchema` and `tomlToMcpServer` follow `i18next.changeLanguage("en")`. Fails 5/6 against the pre-change `provider.ts`, passes after.
- `tests/components/ProviderFormErrorSummary.test.tsx`, `tests/features/providerFormShell.test.ts`: the settingsConfig assertion now matches `/json/i` (the test i18n setup has empty resources, so the message is the key).

**Files**: the files above plus `src/i18n/locales/{en,zh,zh-TW,ja}.json`.

**Commits**: `448299f` (code + tests), deletions in `ed7aac2`, this notes commit.

**Verification**
- `pnpm typecheck`: pass.
- `pnpm test:unit`: 141 files / 1393 tests passed (includes i18n parity / usedKeys / noInlineFallbacks).
- `pnpm format:check`: all files formatted.
- `pnpm lint`: 20 pre-existing `no-app-literal-comparison` errors in untouched files (App.tsx, DeepLinkImportDialog.tsx, ProviderActions.tsx); none in touched files.
- `LC_ALL=C.UTF-8 grep -rnP '[\x{4e00}-\x{9fff}]' src/lib src/utils src/components/providers/forms/hooks --include=*.ts`: non-comment matches remain only in (a) `src/utils/errorUtils.ts` `translateMcpBackendError` and `src/lib/query/queries.ts:161,163`, which match Chinese text emitted by the Rust backend / usage scripts and must stay until the backend returns error codes, and (b) test titles in `src/lib/version.test.ts` (not shipped).

**Remaining (why Partial)**: the grep criterion is not literally clean because of the backend-message matchers in (a); removing them needs structured backend error codes (P2 `IpcError`), which has not landed and is outside this lane (src-tauri/). The test titles in (b) can be translated whenever desired.
