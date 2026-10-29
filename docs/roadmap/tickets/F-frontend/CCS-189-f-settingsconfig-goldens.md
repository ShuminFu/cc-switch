# CCS-189: [provider-form] Add per-app, per-preset settingsConfig golden snapshots

**Status:** Done · **Kind:** chore · **Labels:** `chore`, `phase:F`, `p1-high`, `area:frontend-tests`

> **Ticket key:** `f-settingsconfig-goldens` · **Phase:** F · **Priority:** p1-high · **Effort:** S · **Depends on:** `p0-hotfix-providerform-parse`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
The ProviderForm split is high-risk, and nothing pins the produced settingsConfig.

## Proposed change
For each app and each preset, render the form, submit the defaults, and snapshot the onSubmit payload. Add edit-mode round-trip cases.

## Acceptance criteria / verification
- [x] Snapshots exist for all presets of all 8 apps
- [x] `pnpm test:unit`

## Dependencies
p0-hotfix-providerform-parse

## Implementation notes

**Summary.** `tests/components/providerFormGoldens/` has one test file per app
(claude, claude-desktop, codex, gemini, grokbuild, opencode, openclaw, hermes)
that share `harness.tsx`. Each one covers "custom" and every preset, using the
same preset ids as the form. Each case:

1. renders the real `ProviderForm` in create mode, clicks the preset in the grid
   and fills only what the form needs before it will submit, using fixed values:
   the provider key (`golden-provider`), the API key (`sk-golden-test-key`) and
   empty `required` template inputs (`golden-<input id>`). The snapshot lists
   what was filled under `filled`;
2. submits, clicking "仍要保存" if the soft-issue dialog opens, and snapshots the
   `onSubmit` payload with `settingsConfig` parsed. If the form blocks the
   submit, the snapshot records the toast or invalid-input errors instead;
3. **edit-mode round trip**: it re-opens the payload as `initialData`, saves
   with no changes and requires the same payload back. Create-only bookkeeping
   is ignored: `presetId`, `presetCategory`, `isPartner`,
   `partnerPromotionKey`, `providerKey` and `suggestedDefaults`.

The output is deterministic. MSW answers the commands the form calls on mount
that the shared handlers do not cover (`get_common_config_snippet`,
`auth_get_status` with one signed-in account so Copilot/Codex OAuth presets
submit, `get_hermes_live_provider_ids`, `get_claude_desktop_default_routes`).
`crypto.randomUUID` is stubbed, and the common-config notice is pre-confirmed.
`knownRoundTripDrift` lists presets whose edit save does not yet reproduce the
create payload. For those, the edit payload is snapshotted, and the case fails
once the drift is gone. The only entry is:

- `claude-60` TheRouter: the preset ships both `ANTHROPIC_AUTH_TOKEN` and an
  empty `ANTHROPIC_API_KEY`. Create writes the key to AUTH_TOKEN without saving
  `meta.apiKeyField`, and edit then infers `ANTHROPIC_API_KEY` from the empty
  entry. This needs a follow-up fix in the form or the preset.

The round-trip cases exposed two bugs, fixed in the same commit:

- `GrokBuildProviderForm`: `partnerPromotionKey` state did not start from
  `initialData.meta`, so every edit save dropped `meta.partnerPromotionKey`.
  This broke 37 grokbuild cases before the fix.
- `OmoFormFields`: the `categories = {}` default was a new object on every
  render. OMO Slim passes no categories, so the `[categories]` effect re-ran
  `setState` forever. Selecting the OpenCode "OMO Slim" preset hung the test,
  and the app would get stuck in a render loop the same way. The fix uses a
  stable module-level empty object.

Coverage: 467 cases and 468 snapshots: claude 71, claude-desktop 68, codex 64,
gemini 21, grokbuild 62, opencode 60, openclaw 60, hermes 61. Every preset
submits. Only the two "custom" defaults are pinned as blocked (empty name
and fields), which is the form's intended behaviour.

**Files.**
- `tests/components/providerFormGoldens/harness.tsx`, `mocks.tsx`, `*.test.tsx` (8), `__snapshots__/*.snap` (8)
- `src/components/providers/forms/GrokBuildProviderForm.tsx`
- `src/components/providers/forms/OmoFormFields.tsx`

**Commits.** `1ad2056` (goldens + fixes); this notes edit follows in a separate docs commit.

**Verification.**
- `CI=true npx vitest run tests/components/providerFormGoldens`: 8 files, 467
  tests passed. It ran in CI mode, which writes no snapshots, right after the
  run that wrote them, so the output is stable.
- `CI=true pnpm test:unit`: 104 files, 1139 tests passed (about 210 s on 4
  cores; the goldens add about 130 s of that).
- `pnpm typecheck`: clean. `pnpm format:check`: all files pass.
- Before the fixes: grokbuild had 37 cases failing on the dropped
  `partnerPromotionKey`, and `opencode-58 OMO Slim` hung (the worker never
  finished).
