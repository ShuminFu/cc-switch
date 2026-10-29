# CCS-115: [errors] Switch frontend error handling on IpcError.code

**Status:** Done · **Kind:** refactor · **Labels:** `refactor`, `phase:P2`, `p2-medium`, `area:frontend`

> **Ticket key:** `p2-ipc-error-frontend` · **Phase:** P2 · **Priority:** p2-medium · **Effort:** S · **Depends on:** `p2-ipc-error-type`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
errorUtils and skillErrorParser match localized message substrings, so any rewording breaks them.

## Evidence
- src/utils/errorUtils.ts:53-64; skillErrorParser.ts

## Proposed change
Parse IpcError and switch on code. Keep a legacy string fallback until 100% of commands are migrated.

## Acceptance criteria / verification
- [x] `grep -nP '[\x{4e00}-\x{9fff}]' src/utils/errorUtils.ts` finds no matching code
- [x] Vitest per code
- [x] `pnpm typecheck` and `pnpm test:unit`

## Dependencies
p2-ipc-error-type

## Implementation notes

**Summary.** `translateMcpBackendError` (src/utils/errorUtils.ts) now takes the
raw rejection value and switches on `IpcError.code` via `getIpcErrorCode`
instead of matching Chinese message substrings:
`MCP_VALIDATION -> mcp.error.validationFailed` (new key, all four locales),
`JSON -> mcp.error.jsonInvalid`, `TOML -> mcp.error.tomlInvalid`; every other
code returns `""`. Legacy string fallback: non-`IpcError` rejections (commands
not yet migrated) are never parsed and also return `""`, so callers keep showing
the original text via `extractErrorMessage` (all MCP commands already return
`IpcError` since CCS-114). `McpFormModal` passes the error object, shows the
translated summary as the toast title and the backend message as its
description, and keeps the old raw-text/`saveFailed` behaviour otherwise.
`skillErrorParser.ts` already switched on `code` in CCS-114 (covered by
tests/lib/ipcError.test.ts). The comments in errorUtils.ts were rewritten in
English so the CJK grep is empty, not just free of matching code.

Note: the form already validates id/JSON/TOML/command/url client-side, so the
former fine-grained substring mapping (command/url required) was only a
fallback; `MCP_VALIDATION` carries no sub-reason today, hence one generic
summary plus the backend detail. A structured reason in `details` can be added
backend-side later (e.g. with CCS-122) without touching callers.

**Files.** src/utils/errorUtils.ts, src/components/mcp/McpFormModal.tsx,
src/i18n/locales/{en,zh,zh-TW,ja}.json, tests/utils/errorUtils.test.ts (new),
tests/components/McpFormModal.test.tsx.

**Commits.** see `git log --grep CCS-115` (code and this note in one commit).

**Verification.**
- `LC_ALL=C.UTF-8 grep -nP '[\x{4e00}-\x{9fff}]' src/utils/errorUtils.ts` -> no output (exit 1); 36 matches before.
- `npx vitest run tests/utils/errorUtils.test.ts tests/components/McpFormModal.test.tsx tests/i18n tests/lib/ipcError.test.ts` -> 9 files, 105 tests passed (one case per `IPC_ERROR_CODES` entry, raw payload, reworded message, legacy strings; new McpFormModal regression test for an `IpcError` rejection fails on the old substring code).
- `pnpm typecheck` -> clean; `pnpm test:unit` -> 153 files passed; `pnpm format:check` -> all files formatted; `npx eslint` on touched files -> clean.
- No Rust changes.
