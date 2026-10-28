# CCS-022: [i18n] Translate the 30 missing zh-TW keys and drop 2 unused zh-TW-only keys

**Status:** Open · **Kind:** bug · **Labels:** `bug`, `phase:P0`, `p3-low`, `area:i18n`

> **Ticket key:** `p0-i18n-zhtw-keys` · **Phase:** P0 · **Priority:** p3-low · **Effort:** S
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Verified finding frontend-i18n-zhtw-missing-keys. zh-TW is missing 30 settings.* keys for tool install, update and diagnose. They fall back to Simplified Chinese (the zh-TW -> zh -> en hierarchy), not to English as first claimed. settings.oneClickInstall and providerForm.partnerPromotion.aicoding exist only in zh-TW and are unused.

## Evidence
- src/i18n/locales/zh-TW.json (2555 keys vs 2583)
- Usage: AboutSection.tsx:553,557,669; ToolUpgradeConfirmDialog.tsx

## Proposed change
Add Traditional Chinese translations for the 30 keys and remove the 2 extra keys. No fallbackLng change is needed.

## Acceptance criteria / verification
- [ ] The locale parity test passes
- [ ] In zh-TW, About / tool install shows 安裝, not 安装

## Dependencies
None.
