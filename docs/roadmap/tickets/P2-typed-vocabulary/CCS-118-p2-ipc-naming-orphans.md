# CCS-118: [ipc] Normalize command and argument casing, and wire up or delete orphan commands

**Status:** Done · **Kind:** chore · **Labels:** `chore`, `phase:P2`, `p3-low`, `area:ipc`

> **Ticket key:** `p2-ipc-naming-orphans` · **Phase:** P2 · **Priority:** p3-low · **Effort:** M · **Depends on:** `p2-bindings-generate`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Command names and arguments use mixed casing (queryProviderUsage, testUsageScript, 41 non_snake_case args), and 14 commands are never called, including parse_deeplink.

## Proposed change
Rename through the bindings, then delete the orphans or wire them up (list them in the PR).

## Acceptance criteria / verification
- [ ] A `#![deny(non_snake_case)]` check in commands/ passes
- [ ] An orphan check script reports 0
- [ ] `pnpm typecheck`, `pnpm test:unit`, `cargo test --manifest-path src-tauri/Cargo.toml`

## Dependencies
p2-bindings-generate

## Implementation notes

**Summary.**
- **Command names.** `queryProviderUsage` and `testUsageScript` are now `query_provider_usage` and `test_usage_script`. The changes cover the Rust command fns, `generate_handler!`, the tray caller, `src/lib/api/usage.ts`, the regenerated `src/shared/bindings.ts` and the MSW allowlist.
- **Arguments.** All 37 remaining camelCase command arguments in `commands/` are now snake_case: `providerId`, `addToLive`, `filePath`, `preserveEmptyPassword`, `sessionId` and the rest. Every `#[allow(non_snake_case)]` and `#![allow(non_snake_case)]` was removed. The frontend is unchanged because Tauri maps snake_case args to the same camelCase keys (no command uses `rename_all = "snake_case"`). The 41 in the plan also counted `get_hermes_live_provider`'s `providerId` (that command is deleted) and the two fn names. Shadowing clones in `session_manager.rs` that existed only because of the rename were dropped.
- **Lint.** `commands/mod.rs` now has `#![forbid(non_snake_case)]`. That is `deny` that a submodule cannot override with `allow`.
- **Orphans deleted.** These 12 commands were registered but never invoked by `src/`:
  - `get_claude_config_status` and `get_config_status`
  - `get_copilot_optimizer_config` and `set_copilot_optimizer_config`
  - `get_claude_plugin_status`, `read_claude_plugin_config` and `is_claude_plugin_applied`
  - `import_from_deeplink` (the legacy one; `import_from_deeplink_unified` stays)
  - `get_hermes_live_provider`
  - `enter_lightweight_mode`, `exit_lightweight_mode` and `is_lightweight_mode`. The tray and the lib still call `crate::lightweight::*` directly, and `commands/lightweight.rs` was removed.

  `parse_deeplink` had already been removed before this ticket. Helpers left dead by the deletions were removed too: `config::ConfigStatus`, `config::get_claude_config_status`, and `claude_plugin::{claude_config_status, is_claude_config_applied, is_managed_config}`. `copy_file` and `delete_file` in `config.rs` moved above its test module so that `clippy::items_after_test_module` stays quiet. The Copilot optimizer DAO (`get_/set_copilot_optimizer_config` on `Database`) is kept. The proxy still reads it, so the setting now keeps its default unless a settings UI is added later.
- **Check script.** The new `scripts/check-ipc-orphans.mjs` (with `.d.mts`) parses `generate_handler!` and scans the non-test `src/**/*.ts(x)` files, skipping the generated bindings. It reports:
  - orphans: registered commands that no source names as a string literal
  - unknown `invoke("…")` targets
  - non-snake_case command names

  It exits 1 if any of these is found. `tests/scripts/checkIpcOrphans.test.ts` runs it against the real tree, so `pnpm test:unit` enforces it in CI. Run against the pre-change `lib.rs`, it reported 14 problems: 12 orphans plus the 2 camelCase names.
- **ADR.** Follow-up 3 in `docs/dev/adr/bindings.md` now records that the names are normalised.

**Files:** `src-tauri/src/commands/{mod,provider,config,deeplink,hermes,import_export,mcp,misc,openclaw,plugin,s3_sync,session_manager,settings,webdav_sync}.rs`, `src-tauri/src/commands/lightweight.rs` (deleted), `src-tauri/src/{lib,tray,config,claude_plugin}.rs`, `src/lib/api/usage.ts`, `src/shared/bindings.ts` (regenerated), `tests/msw/unmockedCommands.ts`, `scripts/check-ipc-orphans.mjs` (new), `scripts/check-ipc-orphans.d.mts` (new), `tests/scripts/checkIpcOrphans.test.ts` (new), `docs/dev/adr/bindings.md`.

**Commits:** 304d46d (implementation), plus this ticket-notes commit.

**Verification:**
- `node scripts/check-ipc-orphans.mjs`: `IPC commands: 274 registered, 0 orphans, 0 unknown, 0 non-snake_case`.
- `cargo fmt`: clean.
- `cargo clippy -- -D warnings` and `cargo clippy --all-targets -- -D warnings`: clean, with `forbid(non_snake_case)` active.
- `UPDATE_BINDINGS=1 cargo test --lib bindings`: 2 passed. The only change to `bindings.ts` is the `COMMANDS` list.
- `cargo test --no-fail-fast`: 0 failed in every binary; lib: 2224 passed, 3 ignored.
- `pnpm typecheck`: passes.
- `pnpm test:unit`: 152 files / 1460 tests passed, including the new `checkIpcOrphans` (3) and `tests/msw/handlers.test.ts`.
- `pnpm format:check`: clean.
