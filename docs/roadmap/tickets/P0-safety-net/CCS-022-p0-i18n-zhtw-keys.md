# CCS-022: [i18n] Translate the 30 missing zh-TW keys and drop 2 unused zh-TW-only keys

**Status:** Done · **Kind:** bug · **Labels:** `bug`, `phase:P0`, `p3-low`, `area:i18n`

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
- [x] The locale parity test passes
- [x] In zh-TW, About / tool install shows 安裝, not 安装

## Dependencies
None.

## Implementation notes
- **Summary:** Added Traditional Chinese translations for the 30 missing `settings.*` keys (tool install / update / diagnose / upgrade-confirm strings, interpolation variables preserved) and removed the two unused zh-TW-only keys `settings.oneClickInstall` and `providerForm.partnerPromotion.aicoding` (no code or preset references them). zh-TW now has 2596 keys, identical to en/ja/zh. Added a vitest locale parity test that checks ja/zh/zh-TW against en for missing keys, extra keys and matching `{{placeholders}}`, plus a spot check that zh-TW `settings.toolInstall` is `安裝`. No `fallbackLng` change.
- **Files:** `src/i18n/locales/zh-TW.json`, `tests/i18n/localeParity.test.ts` (new).
- **Commits:** `1924827` (fix), plus this ticket-notes commit.
- **Verification:**
  - `npx vitest run tests/i18n/localeParity.test.ts` against the pre-change zh-TW.json: 3 failed (zh-TW missing keys, extra keys, Traditional Chinese check) / 7 passed; after the change: all pass.
  - `pnpm typecheck`: passes.
  - `pnpm test:unit`: 76 files, 518 tests passed.
  - `pnpm format:check`: all files use Prettier style.
