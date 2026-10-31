# CCS-253: [i18n] Remove the dead Chinese usage-script matchers from queries.ts and close CCS-193

**Status:** Done · **Kind:** bug · **Labels:** `bug`, `phase:F`, `p3-low`, `area:i18n`

> **Ticket key:** `f-i18n-usage-cjk-matchers` · **Phase:** F · **Priority:** p3-low · **Effort:** S
> Plan: [next-phase-plan.md](../../next-phase-plan.md)
> Found during the Partial-ticket triage at d962272 (CCS-193, CCS-007). Source tickets: [CCS-193](../F-frontend/CCS-193-f-i18n-validation-messages.md), [CCS-007](../F-frontend/CCS-007-epic-f.md).

## Problem
CCS-193 (and the CCS-007 epic note) stayed Partial on the grep criterion, waiting for backend IpcError codes. That has landed (CCS-114/115), and `translateMcpBackendError`'s Chinese matchers are gone. The only non-comment CJK left is `isTransientUsageError` in `src/lib/query/queries.ts:161` (`e.includes("请求失败")`) and `:163` (`e.includes("读取响应失败")`). These are dead: `src-tauri/src/services/provider/usage.rs:63-69` returns `Err` with an i18nKey for `usage_script.request_failed` / `usage_script.read_response_failed`, so the Chinese strings (`usage_script.rs:305,314`) never reach a `success:false` `UsageResult.error`; transient script failures arrive as rejections handled by `rejected: true` in `resolveDisplayUsage`.

## Proposed change
- Delete the two zh `includes` lines (and the en `request failed` / `failed to read response` lines, or keep them as a documented defensive fallback).
- Update the comment block (~140-157) to say script transport failures arrive as rejections keyed by i18nKey.
- Update `tests/lib/keepLastGoodUsage.test.ts:29,32` to drop the Chinese cases.
- Optionally add a vitest guard that scans `src/lib`, `src/utils` and `src/components/providers/forms/hooks` `*.ts` for CJK outside comments.
- Re-verify CCS-193 criteria 1 and 3, tick all three boxes, set Done, and add a Review note that the blocker cleared.

## Acceptance criteria / verification
- [x] `LC_ALL=C.UTF-8 grep -rnP '[\x{4e00}-\x{9fff}]' src/lib src/utils src/components/providers/forms/hooks --include=*.ts | grep -vP '^\S+:\d+:\s*(//|\*|/\*)|//'` prints nothing.
- [x] `pnpm vitest run tests/lib/validationMessagesI18n.test.ts tests/lib/keepLastGoodUsage.test.ts` passes.
- [x] `pnpm typecheck` and `pnpm test:unit` pass.

## Dependencies
None.

## Implementation notes

**Summary**
- `src/lib/query/queries.ts`: removed the two zh matchers (`请求失败`, `读取响应失败`) from `isTransientUsageError`. The English `failed to read response` matcher stays because it is live: the native balance/coding_plan paths still return `Failed to read response: …` as `success:false`. `request failed` stays as a documented defensive fallback. The comment block now says usage-script transport failures arrive as rejections keyed by i18nKey (`usage_script.request_failed` / `usage_script.read_response_failed`) and are handled by the `rejected` branch of `resolveDisplayUsage`.
- `tests/lib/keepLastGoodUsage.test.ts`: dropped the two Chinese cases.
- `tests/lib/validationMessagesI18n.test.ts`: new guard that strips comments from every `*.ts` under `src/lib`, `src/utils` and `src/components/providers/forms/hooks` and fails on any remaining CJK. It fails against the pre-change `queries.ts` (reports `e.includes("请求失败")`) and passes after. Unlike the ticket's grep, it also catches CJK in code lines that carry a trailing `//` comment.
- CCS-193 re-verified, all three boxes ticked, set to Done with a Review note. CCS-007 epic notes updated (CCS-193 row and remaining item 4).

**Files**: `src/lib/query/queries.ts`, `tests/lib/keepLastGoodUsage.test.ts`, `tests/lib/validationMessagesI18n.test.ts`, `docs/roadmap/tickets/F-frontend/{CCS-253-f-i18n-usage-cjk-matchers,CCS-193-f-i18n-validation-messages,CCS-007-epic-f}.md`.

**Commits**: `d8bfde7` (code + tests), plus this docs commit.

**Verification**
- Acceptance grep (`LC_ALL=C.UTF-8 grep -rnP '[\x{4e00}-\x{9fff}]' … | grep -vP '^\S+:\d+:\s*(//|\*|/\*)|//'`): prints nothing.
- `pnpm vitest run tests/lib/validationMessagesI18n.test.ts tests/lib/keepLastGoodUsage.test.ts`: 2 files / 28 tests pass.
- `pnpm typecheck`: pass. `pnpm test:unit`: 179 files / 1787 tests pass. `pnpm format:check`: clean.
