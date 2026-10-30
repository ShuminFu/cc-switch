# CCS-152: [takeover] Add crash/restore E2E for each proxy-capable app and a legacy-path runtime flag

**Status:** Partial · **Kind:** chore · **Labels:** `chore`, `phase:P4`, `p1-high`, `area:proxy-tests`

> **Ticket key:** `p4-takeover-crash-e2e` · **Phase:** P4 · **Priority:** p1-high · **Effort:** M · **Depends on:** `p0-proxy-harness-e2e`, `p0-config-writer-snapshots`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Takeover consolidation can corrupt users' settings.json, config.toml or auth.json. It needs a gate in place before any code moves.

## Proposed change
- For each of claude, codex, gemini and grokbuild: apply takeover, kill the process (simulated drop without cleanup), restart, and assert the live files are restored byte-for-byte.
- Add a `proxy.takeoverEngine=legacy` runtime flag for one release.

## Acceptance criteria / verification
- [ ] Green on 3 OSes
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml --test takeover_crash`

## Dependencies
p0-proxy-harness-e2e, p0-config-writer-snapshots

## Implementation notes

**Summary.** `src-tauri/tests/takeover_crash.rs` is the crash/restore gate for the takeover consolidation (CCS-153 / CCS-154). For each of claude, codex, gemini and grokbuild, and through both `start_with_takeover` and `set_takeover_for_app`, it does the following:

1. Seeds the live files and a matching current provider in an isolated HOME, using an on-disk `Database::init()`.
2. Applies takeover and asserts the live files carry the placeholder and a backup exists.
3. "Kills the process": drops the tokio runtime that runs the proxy server, the `AppState` and the DB handle without calling `stop`, `stop_with_restore` or `cleanup_before_exit`.
4. Restarts: reopens the same DB and runs the startup recovery from `lib.rs` (`has_any_live_backup` / `detect_takeover_in_live_configs` then `recover_from_crash`).
5. Asserts that every file under HOME (excluding `~/.cc-switch`) matches the pre-takeover bytes, and that a second restart recovers nothing and touches no file.

**Finding: the old restore path was not byte-exact.** Restoring from the JSON backup re-serialised hand-written files. Claude `settings.json` and Codex `auth.json` lost their indentation and layout, and Gemini `.env` lost its final newline. Only files that CC Switch had written itself came back byte-for-byte.

**Fix: raw snapshots of the live files.**
- `services/takeover_snapshot.rs` adds `RawLiveSnapshot`. `ProxyService::save_takeover_backup` replaces the five inline `save_live_backup` calls in `backup_live_configs` and `backup_live_config_strict`.
- When the backup is saved, it also stores the original text of the app's live files in the new column `proxy_live_backup.raw_snapshot`. That column comes from schema v21, an additive migration (`database/migrations/v021.rs`).
- The snapshot is stored only if re-reading the live config still equals the backup, so a file edited mid-backup is not captured.
- After restore writes the backup (`restore_live_config_for_app_inner` and the backup branch of `restore_live_config_for_app_with_fallback_inner`), `restore_takeover_raw_snapshot` writes the original bytes back atomically. It only writes paths that are still the app's current live paths.
- `save_live_backup` uses `INSERT OR REPLACE`. Any later rewrite of the backup row (hot switch, provider save while taken over, hot-switch rollback) therefore clears the stale snapshot, and restore falls back to the JSON-only path. A test covers this.

**Runtime flag.**
- `AppSettings.takeover_engine` is stored as the top-level `takeoverEngine` key in `~/.cc-switch/settings.json`, next to `codecEngine`. As with `codecEngine`, it is not nested as `proxy.takeoverEngine`.
- Values: `"unified"` is the default and is omitted from the file. `"legacy"` neither records nor uses snapshots, so takeover and restore behave exactly as before this change.
- Parsing is lenient: unknown or non-string values fall back to `unified` with a warning.
- Accessor: `settings::takeover_engine()`. `takeoverEngine?` was added to the TS `Settings` type so the frontend keeps the field when it saves the full settings.
- CCS-153 / CCS-154 should put the consolidated engine behind `Unified` and keep `Legacy` on the current code for one release.

**Tests.**
- `takeover_crash.rs`:
  - `crash_after_takeover_restores_hand_written_live_files_byte_for_byte`: 4 apps × 2 entry points. This is the regression test: under `legacy` the Claude, Codex and Gemini files are not byte-identical, and `legacy_engine_restores_hand_written_settings_but_not_their_formatting` records that for Claude.
  - `crash_after_takeover_restores_cc_switch_written_live_files_byte_for_byte`
  - `legacy_engine_restores_cc_switch_written_live_files_byte_for_byte`
  - `legacy_engine_restores_hand_written_settings_but_not_their_formatting`: checks semantic equality per app.
  - `takeover_snapshot_is_dropped_when_the_backup_is_rewritten`
- Unit tests in `takeover_snapshot.rs`: flag parsing, serde shape, snapshot round-trip.
- `migrate_v20_to_v21_adds_live_backup_raw_snapshot`.
- New DB fixture `tests/fixtures/db/v21.sql`, generated with `scripts/gen-db-fixture.sh` from the commit's tree using `CCS_FIXTURE_LOCKFILE=current`.

**Files.** `src-tauri/tests/takeover_crash.rs` (new), `src-tauri/src/services/takeover_snapshot.rs` (new), `src-tauri/src/services/mod.rs`, `src-tauri/src/services/proxy.rs`, `src-tauri/src/settings.rs`, `src-tauri/src/database/dao/proxy.rs`, `src-tauri/src/database/migrations/{mod.rs,v021.rs,tests.rs}`, `src-tauri/src/database/mod.rs` (`SCHEMA_VERSION = 21`), `src-tauri/tests/fixtures/db/v21.sql` (new), `src/types.ts`. Nothing under `src-tauri/src/proxy/`, `commands/**`, `lib.rs` or `tray.rs` was touched.

**Commits.** `b144fd3` test(proxy): crash/restore E2E for each takeover app and a takeoverEngine flag (CCS-152). This ticket record and the fixture's `source` header follow as a separate docs commit.

**Verification (Linux).**
- `cargo test --manifest-path src-tauri/Cargo.toml --test takeover_crash`: 5 passed.
- `cargo fmt`: clean. `cargo clippy --all-targets -- -D warnings`: clean.
- `cargo test --manifest-path src-tauri/Cargo.toml --no-fail-fast`: 3011 passed, 0 failed. This includes the lib suite (2774 passed, 3 ignored), `migrations` (v21 fixture), `app_adapter_conformance` and `proxy_e2e`.
- `pnpm typecheck`: clean. `pnpm test:unit`: 163 files, 1567 tests passed. Prettier check on `src/types.ts`: clean.

**Remaining.** The "Green on 3 OSes" item is not verified: it needs the CI matrix (`ci.yml` runs the full `cargo test` on ubuntu-22.04, windows-latest and macos-latest, and that run includes `takeover_crash`). The suite uses only the `TestEnv` path injection, `config_dir()` from the adapters and ephemeral ports, so nothing in it is specific to one OS. It was only run on Linux here.

**Review.** I checked the commits against the spec, the restore and backup call sites, sync import of `proxy_live_backup` (the temp DB is migrated before local rows are copied back, so the new column is safe) and the file permissions on restore (`atomic_write` keeps the existing mode). Everything holds up. I also ran an extra probe: Codex with only `config.toml` and no `auth.json` also came back byte-for-byte. One defect fixed: if the snapshot failed to serialize (for example, a path that is not UTF-8), `save_takeover_backup` returned an error and blocked takeover. The code meant to only skip the snapshot, so it now logs a warning and continues. Known limitation, left as is: the hot-switch rollback (`rollback_hot_switch_preparation`) writes the previous backup back with `save_live_backup`, which drops the snapshot. After a failed hot switch, restore therefore falls back to the JSON backup: the settings are correct but the formatting may not be. Re-verified on Linux: `takeover_crash` 5 passed; full `cargo test --no-fail-fast` 3011 passed, 0 failed; fmt and clippy (`--all-targets -D warnings`) clean; `pnpm typecheck` clean. Status stays Partial until a 3-OS CI run is green.

### Remaining after triage

- Blocked: Green on 3 OSes (ubuntu-22.04, windows-latest, macos-latest). Needs a windows-latest/macos-latest run of the ci.yml backend matrix, or real machines (cross-compiling to windows-gnu/apple fails on aws-lc-sys). Linux is 10/10 locally.
- Follow-up [CCS-221](../P0-safety-net/CCS-221-p0-docs-ticket-evidence-refresh.md): Tick already-verified acceptance boxes and refresh stale evidence in Partial tickets.
