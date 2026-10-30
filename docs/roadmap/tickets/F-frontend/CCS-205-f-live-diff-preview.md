# CCS-205: [provider-form] Preview the diff of the target live config before saving

**Status:** Done · **Kind:** feature · **Labels:** `feature`, `phase:F`, `p3-low`, `area:frontend`

> **Ticket key:** `f-live-diff-preview` · **Phase:** F · **Priority:** p3-low · **Effort:** M · **Depends on:** `f-providerform-shell`, `p4-adapter-additive`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Users save provider edits without seeing what will change in ~/.claude/settings.json, config.toml and the other live files.

## Proposed change
Add a backend command preview_live_write(app, provider) that returns the adapter output without writing it. The form shows a side-by-side diff before Save.

## Acceptance criteria / verification
- [x] Rust test: preview equals the bytes later written
- [x] RTL: the diff renders
- [x] `pnpm test:unit` and `cargo test --manifest-path src-tauri/Cargo.toml preview`

## Dependencies
f-providerform-shell, p4-adapter-additive

## Implementation notes

**Summary.** New Tauri command `preview_live_write(app, provider, originalId?)`
→ `ProviderService::preview_live_write`. It applies the same normalisation,
validation and "does this save write live?" decision as `update`/`add`
(switch-mode: only the effective current provider; additive: only providers
already in live; OMO: only the current variant; proxy takeover with a live
backup: no live write), then runs the real `write_live_with_common_config`
path — plus the MCP re-projection that a normal save does — inside
`config::capture_writes`. While capturing, `atomic_write`/`atomic_write_secret`/
`write_json_file`/`write_text_file`/`delete_file` only record `(path, bytes)` on
the current thread; nothing touches disk. The result is
`LiveWritePreview { writesLive, skipReason, files: [{ path, before, after }] }`
(one entry per path, final content). Because the adapter code itself produces
the bytes, the preview is exactly what the save writes.

Supporting changes so the captured run is faithful and side-effect free:
- `config::read_text_file_if_exists` returns captured pending contents during a
  capture; used where a write flow re-reads its own output (Gemini
  `security.auth.selectedType` after writing `settings.json`, Codex/Grok
  `config.toml` and Gemini `settings.json` MCP upsert/remove).
- Hermes/OpenClaw skip creating and pruning config backups while capturing;
  Gemini `.env` skips its chmod/mkdir; `write_json_file`/`write_text_file` skip
  `create_dir_all`.
- MCP re-projection writes to files the provider write did not touch (e.g.
  Claude's `~/.claude.json`) are left out of the preview.

Frontend: `providersApi.previewLiveWrite`; `useLiveDiffPreview` +
`LiveDiffPreviewDialog` (side-by-side table per changed file, LCS line diff
with 3-line context collapsing in `liveDiff.ts`). `EditProviderDialog` calls it
on submit: when the save writes live and some file changes, the diff opens and
Save happens only after "Save"; "Back to editing" keeps the form. No live
write, no changes, or a preview error → saves directly as before. i18n keys
`providerForm.liveDiff.*` in en/zh/zh-TW/ja. The DTOs are exported to
`src/shared/bindings.ts` (`.typ::<LiveWritePreview>()`; the command itself is
not added to `collect_commands!`, which is still usage-only).

Known limits: Claude Desktop proxy mode may create the gateway token in the DB
during preview (same token the save then uses); the preview runs on the
current live files, so a concurrent external edit between preview and save
would change the result.

**Files.** `src-tauri/src/config.rs`, `src-tauri/src/services/provider/preview.rs`
(new), `src-tauri/src/services/provider/mod.rs`, `src-tauri/src/commands/provider.rs`,
`src-tauri/src/lib.rs`, `src-tauri/src/bindings.rs`, `src-tauri/src/apps/gemini/config.rs`,
`src-tauri/src/apps/hermes/config.rs`, `src-tauri/src/apps/openclaw/config.rs`,
`src-tauri/src/gemini_mcp.rs`, `src-tauri/src/mcp/codex.rs`, `src-tauri/src/mcp/grokbuild.rs`,
`src-tauri/tests/live_write_preview.rs` (new), `src/features/providers/forms/liveDiff.ts`
(new), `src/features/providers/forms/LiveDiffPreviewDialog.tsx` (new),
`src/components/providers/EditProviderDialog.tsx`, `src/lib/api/providers.ts`,
`src/shared/bindings.ts` (regenerated), `src/i18n/locales/{en,zh,zh-TW,ja}.json`,
`tests/components/EditProviderDialog.test.tsx`, `tests/features/liveDiff.test.ts` (new),
`tests/msw/handlers.ts`, `tests/msw/commands.ts`.

**Commits.** b6e6327 (implementation); this ticket update in a follow-up commit.

**Verification.**
- `cargo test --manifest-path src-tauri/Cargo.toml preview` → 15 passed, 0 failed
  (incl. `live_write_preview`: Claude, Codex with MCP re-projection, Gemini
  read-after-write, Hermes without backups — each asserts the preview leaves
  HOME untouched and that the bytes on disk after `update` equal the preview's
  `after`; plus non-current provider → `notCurrent`; and `config::tests::capture_writes_*`).
  Disabling the captured-read overlay makes the Codex and Gemini cases fail.
- `cargo test --manifest-path src-tauri/Cargo.toml --no-fail-fast` → 2535 passed, 0 failed.
- `cargo test --lib` → 2356 passed; `cargo fmt` clean; `cargo clippy -- -D warnings`
  and `cargo clippy --tests -- -D warnings` clean.
- `pnpm typecheck` clean; `pnpm test:unit` → 156 files / 1521 tests passed
  (RTL: `EditProviderDialog` renders the side-by-side diff for the changed file,
  waits for confirm, "back" cancels, preview failure saves directly;
  `liveDiff.test.ts` unit tests); `pnpm format:check` clean; eslint on touched
  files clean.

**Review.** One missed read-after-write site: Claude Desktop "restore official"
writes `deploymentMode: "1p"` to the 3P `claude_desktop_config.json` and then
re-reads that file from disk to strip `enterpriseConfig`. During a preview the
re-read saw the old file, so when `enterpriseConfig` was present the preview
showed `"3p"` while the save writes `"1p"`. `read_json_or_empty` in
`apps/claude_desktop/config.rs` now goes through `config::read_text_file_if_exists`
(test `claude_desktop_restore_preview_matches_real_restore`, which fails without
the fix). The `create_dir_all` calls in the Codex `auth.json`/`config.toml`
writer, the Hermes `config.yaml` writer and the Gemini MCP `settings.json`
writer are skipped while capturing, so a preview no longer creates directories.
Known limit, not changed: during proxy takeover of Claude/Codex the preview
reports `proxyTakeover` and the form saves without a diff, although with the
proxy running the save also rewrites the proxy-safe live file
(`refresh_live_after_provider_save`). Previewing that path would need the live
backup update, a DB write, to run first, so it stays out of the preview.
Re-verified: `cargo test --no-fail-fast` all passed, `cargo clippy` (lib and
`--tests`) with `-D warnings` clean, `cargo fmt` clean, `pnpm typecheck` clean,
`pnpm test:unit` passed.
