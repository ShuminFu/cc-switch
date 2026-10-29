# CCS-072: [deeplink] Use one masking helper that shows at most 4 characters

**Status:** Done · **Kind:** security · **Labels:** `security`, `phase:P1`, `p3-low`, `area:deeplink`

> **Ticket key:** `p1-deeplink-mask-value` · **Phase:** P1 · **Priority:** p3-low · **Effort:** S
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Verified finding frontend-deeplink-masking-weak. maskValue shows 8 characters, and shows sensitive values of 8 characters or fewer in full. The apiKey mask shows 4.

## Evidence
- src/components/DeepLinkImportDialog.tsx:281-290 vs 209-213; used at 538, 565, 601

## Proposed change
One shared helper: the first 4 characters plus fixed asterisks, or "****" for values of 4 characters or fewer.

## Acceptance criteria / verification
- [x] Vitest: maskValue('ANTHROPIC_AUTH_TOKEN','abcdefgh') == '****'; 'sk-abcdefghijk' gives 'sk-a' plus asterisks; non-sensitive keys are unchanged
- [x] `pnpm typecheck` and `pnpm test:unit`

## Dependencies
None.

## Implementation notes
- **Summary:** Added one shared helper, `src/lib/deeplink/mask.ts` (`maskSecret`, `isSensitiveKey`, `maskValue`). `DeepLinkImportDialog` uses it for both the apiKey line and the sensitive env/auth values (keys that contain TOKEN/KEY/SECRET/PASSWORD, case-insensitive). A secret shows its first 4 characters plus a fixed 20 asterisks, so the length stays hidden. Values of 8 characters or fewer (and empty or missing values) show as `****`. Non-sensitive keys are not changed.
- **Threshold choice:** The proposed change says `****` applies to values of 4 characters or fewer. The acceptance criterion requires `maskValue('ANTHROPIC_AUTH_TOKEN','abcdefgh') == '****'`, though, so the cut-off is 8 characters. That way a short secret is never half exposed. The apiKey mask is stricter as a result: before, it revealed 4 characters for anything longer than 4.
- **Files:** `src/lib/deeplink/mask.ts` (new), `src/components/DeepLinkImportDialog.tsx`, `tests/lib/deeplinkMask.test.ts` (new), `tests/components/DeepLinkImportDialog.test.tsx`.
- **Commits:** f56cd3b (code + tests), plus this ticket update.
- **Verification:**
  - `npx vitest run tests/lib/deeplinkMask.test.ts tests/components/DeepLinkImportDialog.test.tsx` gave 12/12 passed. The new component test `masks short and long env secrets to at most 4 characters` fails against the old dialog (1 failed / 6 passed) and passes after the change.
  - `pnpm typecheck` passed.
  - `pnpm test:unit` gave 122 files / 1245 tests passed.
  - `pnpm format:check` passed.
