# CCS-021: [i18n] Add 10 keys that are used in code but missing from every locale

**Status:** Done · **Kind:** bug · **Labels:** `bug`, `phase:P0`, `p2-medium`, `area:i18n`

> **Ticket key:** `p0-i18n-missing-keys` · **Phase:** P0 · **Priority:** p2-medium · **Effort:** S
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Verified finding frontend-i18n-missing-keys. English and Japanese users see the Chinese defaultValue.

## Evidence
- GrokBuildProviderForm.tsx:304 providerForm.requiredFields
- useCodexCommonConfig.ts:324 codexConfig.noCommonConfigToApply
- ProviderForm.tsx:1071/1100/1126 providerForm.providerKeyStatusLoading
- RequestLogTable.tsx:280 and RequestDetailPanel.tsx:289 usage.unpriced
- FailoverToggle.tsx:46 failover.tooltip.takeoverRequired
- ClaudeDesktopRouteToggle.tsx:45 claudeDesktop.route.stopBlockedByTakeover
- useProxyStatus.ts:72/84 proxy.server.stopped/stopFailed
- useProviderActions.ts:218 notifications.proxyReasonClaudeDesktop
- App.tsx:757 provider.duplicateLiveIdsLoadFailed

## Proposed change
Add all 10 keys, keeping their interpolation variables, to en, ja, zh and zh-TW. Use the existing Chinese defaults for zh and zh-TW.

## Acceptance criteria / verification
- [ ] `jq -r 'paths(scalars)|join(".")'` on each locale lists all 10 keys
- [ ] With the app in English, stopping the proxy shows an English toast
- [ ] p0-ci-i18n-parity passes for these keys

## Dependencies
None.

## Implementation notes

**Summary.** Added the 10 keys to `en`, `ja`, `zh` and `zh-TW`. zh uses the existing Chinese `defaultValue` from the code. zh-TW is a Traditional Chinese conversion of it, using the terms already in that locale (設定, 供應商標識, 載入). en/ja follow the wording of sibling keys ("routing service", "Provider key", "routing/proxy takeover", 「ルーティングサービス」「プロバイダーキー」「引き継ぎ」). Interpolation variables are kept: `failover.tooltip.takeoverRequired` → `{{app}}`, `proxy.server.stopFailed` → `{{detail}}`. `notifications.proxyReasonClaudeDesktop` is phrased as a clause that fits into `notifications.proxyRequiredForSwitch`'s `{{reason}}`, the same way the other `proxyReason*` keys are. Every key is placed next to its siblings under an existing parent object. No other keys changed.

**Files**
- `src/i18n/locales/en.json`, `ja.json`, `zh.json`, `zh-TW.json`
- `tests/i18n/missingKeys.test.ts` (new): for each locale and key, checks that the key resolves to a non-empty string with exactly the expected `{{…}}` placeholders, and that en/ja are not left as the Chinese text. It fails on the previous locale files because every key was missing there.

**Commits:** `939dcf3` (code + test). This ticket-file update is a follow-up commit.

**Verification**
- `jq -r 'paths(scalars)|join(".")'` on each locale, grepping for the 10 keys: en 10, ja 10, zh 10, zh-TW 10 (on `HEAD~` the same check found 0 in every locale).
- `npx vitest run tests/i18n`: 41/41 passed.
- `pnpm test:unit`: 75 files, 508 tests passed.
- `pnpm typecheck`: clean. `pnpm format:check`: clean.
- English proxy-stop toast: `useProxyStatus.ts` calls `t("proxy.server.stopped")`, which now resolves to "Routing service stopped" from `en.json` (i18next only falls back to `defaultValue` when the key is missing). This was checked through the resource test, not by driving the GUI.
- `p0-ci-i18n-parity`: that ticket has not landed yet, so its check can't run here. All four locales now contain these 10 keys with matching placeholders, so they won't produce parity gaps.
