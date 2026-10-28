# CCS-021: [i18n] Add 10 keys that are used in code but missing from every locale

**Status:** Open · **Kind:** bug · **Labels:** `bug`, `phase:P0`, `p2-medium`, `area:i18n`

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
