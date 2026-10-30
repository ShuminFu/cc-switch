# CCS-253: [i18n] Remove the dead Chinese usage-script matchers from queries.ts and close CCS-193

**Status:** Open · **Kind:** bug · **Labels:** `bug`, `phase:F`, `p3-low`, `area:i18n`

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
- [ ] `LC_ALL=C.UTF-8 grep -rnP '[\x{4e00}-\x{9fff}]' src/lib src/utils src/components/providers/forms/hooks --include=*.ts | grep -vP '^\S+:\d+:\s*(//|\*|/\*)|//'` prints nothing.
- [ ] `pnpm vitest run tests/lib/validationMessagesI18n.test.ts tests/lib/keepLastGoodUsage.test.ts` passes.
- [ ] `pnpm typecheck` and `pnpm test:unit` pass.

## Dependencies
None.
