# CCS-170: [apps] Keep a live-config journal with one-click undo of the last switch

**Status:** Done · **Kind:** feature · **Labels:** `feature`, `phase:P4`, `p3-low`, `area:apps`

> **Ticket key:** `p4-live-config-journal` · **Phase:** P4 · **Priority:** p3-low · **Effort:** M · **Depends on:** `p4-adapter-additive`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Backend idea 3 and frontend idea 7. A bad switch cannot be undone.

## Proposed change
- Keep the last N LiveSnapshot entries per app (0600, under app_config_dir/journal).
- Add a revert_last_switch command and an Undo action on the switch toast.

## Acceptance criteria / verification
- [x] Test: switch A to B, revert, and the live files are byte-identical to A
- [x] The journal is bounded
- [x] `cargo test --manifest-path src-tauri/Cargo.toml journal` and `pnpm test:unit`

## Dependencies
p4-adapter-additive

## Implementation notes

**Summary.** `ProviderService::switch_normal` (the path that writes live files, used by the UI, tray, deeplink and profile switches) now runs inside `config::record_writes`. That is a thread-local recorder hooked into `atomic_write*` / `delete_file`, which keeps the original bytes of each path before its first real write. Preview capture mode is not recorded. After a successful switch, the files that actually changed are stored as a `LiveSnapshot` JSON entry (base64 bytes, or `null` for "did not exist") under `<app_config_dir>/journal/<app>/`. The directory is 0700 and entries are written with `atomic_write_secret` (0600). Files inside CC Switch's own dir are left out, because the current provider is restored semantically instead. The journal is capped at `JOURNAL_LIMIT = 10` entries per app, and the oldest are pruned on each write. `SwitchResult` gains `undoable`.

The new `revert_last_switch` command takes the per-app switch lock and restores the newest entry byte for byte. Files the switch created are deleted. It then restores the previous current provider (local settings + DB), or, for additive apps, the target's previous `live_config_managed` flag, and consumes the entry. It refuses to run when the journal is empty, while proxy takeover owns live, or when the current provider changed after that switch (stale entry). In those cases the entry is kept. Hot switches during takeover and OMO switches are not journaled. The switch success toast shows an Undo action when `undoable` is true. Undo invalidates the provider graph, refreshes the tray, and toasts success or the error.

**Files.** `src-tauri/src/config.rs` (`record_writes`, `PriorContent`), `src-tauri/src/services/provider/journal.rs` (+ `journal/tests.rs`), `src-tauri/src/services/provider/mod.rs`, `src-tauri/src/commands/provider.rs`, `src-tauri/src/lib.rs`, `src-tauri/src/bindings.rs`, `src/shared/bindings.ts`, `src/lib/api/providers.ts`, `src/hooks/useProviderActions.ts`, `src/i18n/locales/{en,zh,zh-TW,ja}.json`, `tests/hooks/useProviderActions.test.tsx`, `tests/msw/handlers.ts`.

**Commits.** `17718ce` feat(apps): keep a live-config journal with undo of the last switch (CCS-170); this notes commit follows.

**Verification.**
- `cargo test --manifest-path src-tauri/Cargo.toml --lib journal`: 13 passed. This includes the new `journal_switch_then_revert_restores_claude_live_byte_for_byte` (hand-formatted A live → B → revert gives identical bytes), `journal_revert_restores_every_codex_file_and_removes_created_ones`, `journal_is_bounded_and_reverts_newest_first` (15 switches, 10 entries, each revert gives the exact prior bytes), `journal_entries_are_owner_only` (0700/0600), `journal_revert_refuses_when_current_provider_changed_since`, `journal_revert_restores_additive_live_and_managed_flag` and `config::tests::record_writes_journal_keeps_first_prior_content_and_still_writes`.
- `cargo test --manifest-path src-tauri/Cargo.toml --no-fail-fast`: 2660 passed, 0 failed.
- `cargo fmt --check` clean; `cargo clippy -- -D warnings` clean; `UPDATE_BINDINGS=1 cargo test --lib bindings` regenerated `src/shared/bindings.ts` (`revert_last_switch` in COMMANDS, `RevertSwitchResult` type).
- `pnpm typecheck` clean; `pnpm format:check` clean; `pnpm test:unit`: 160 files / 1549 tests passed. This includes two new `useProviderActions` cases: Undo action reverts and toasts; a failed undo reports the error; a switch that was not journaled has no Undo. One run in parallel with cargo hit a 5 s timeout in `tests/integration/App.test.tsx`; it passed when rerun on its own and in a full rerun.

**Review.** I checked the acceptance criteria against `17718ce` and confirmed them: byte-identical revert, a journal bounded at 10, and 0700/0600 permissions. I re-ran the suites and fixed three gaps.
1. If the provider that was current before the switch was deleted later, the revert still restored its live files. It then cleared only the local current provider, so the effective current provider fell back to the DB's `is_current`, which still pointed at the target. The UI then showed B as current while live held A. The revert now refuses with `switch.revertPreviousMissing` and keeps the entry.
2. A switch made when no provider was current was journaled, but its revert cannot clear DB `is_current`. It hit the same mismatch. Such switches are no longer journaled, so `undoable` is false for them.
3. Undo did not re-sync the Claude plugin config (`~/.claude/config.json`, written by the frontend outside the journal). It now re-applies the plugin config for the restored provider.

New tests: `journal_revert_refuses_when_previous_provider_was_deleted`, `journal_skips_switch_without_previous_current_provider`, and a plugin-sync assertion in the `useProviderActions` Undo case. Results: `cargo test --no-fail-fast` 2662 passed / 0 failed; fmt and clippy clean; `pnpm typecheck` and `format:check` clean; `pnpm test:unit` 160 files / 1549 tests passed.
