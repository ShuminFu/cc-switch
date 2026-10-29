# CCS-075: [ipc] Validate path arguments of privileged commands against allowed roots or dialog-issued tokens

**Status:** Done · **Kind:** security · **Labels:** `security`, `phase:P1`, `p1-high`, `area:ipc`

> **Ticket key:** `p1-ipc-path-validation` · **Phase:** P1 · **Priority:** p1-high · **Effort:** M
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Several commands accept arbitrary filesystem paths from the webview. The partially-real finding ci-ipc-commands-open shows restore_env_backup can append `export NAME=VALUE` to a source_path named in an attacker JSON.

## Evidence
- commands/env.rs:20 restore_env_backup (env_manager.rs:153-166,200-222); import_export.rs:21-60; skill.rs:326 install_skills_from_zip; settings.rs:296 set_app_config_dir_override; pattern to follow: session_manager/mod.rs:131

## Proposed change
- Add a `PickedPath(Uuid)` registry: the dialog plugin's result is registered on the backend, and commands accept the token.
- Alternatively validate against allowed roots (the app backup dir, known shell rc files).
- Inventory TS callers by grep, because bindings do not exist yet.

## Acceptance criteria / verification
- [ ] One test per command: a path outside the allowed roots is rejected; a valid picked path works
- [ ] restore_env_backup rejects a source_path that is not an rc file
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml` and `pnpm test:unit`

## Dependencies
None.

## Implementation notes

**Summary.** Chose a hybrid of the two proposed approaches that keeps the TS API unchanged (no bindings exist yet, and every caller already obtains its path from a backend dialog command):

- New `src-tauri/src/picked_paths.rs`: a bounded in-process registry of paths returned by the backend dialog commands, keyed by purpose (`SqlImport`, `SqlExport`, `SkillZip`, `Directory`). `open_file_dialog`, `save_file_dialog`, `open_zip_file_dialog` and `pick_directory` register the path they return. A path is only accepted by the command matching the dialog that produced it.
- `export_config_to_file` / `import_config_from_file` only accept paths from `save_file_dialog` / `open_file_dialog`; `install_skills_from_zip` only accepts paths from `open_zip_file_dialog`. Logic moved into small testable helpers (`export_sql_to_picked_path`, `import_sql_from_picked_path`, `install_skills_from_picked_zip`).
- `set_app_config_dir_override` (allowed roots, since the field can also be typed): accepts empty/None (reset), a `pick_directory` result, the currently active override, or an absolute path under the home dir (`~` supported) without `..`/`.` components. Anything else must be chosen with the directory picker.
- `restore_env_backup`: the backup must canonicalize to an `env-backup-*.json` file inside `~/.cc-switch/backups`; every entry is validated before any write: `file` entries must target one of the rc files the env checker scans (`env_checker::shell_config_files()`), variable names must be shell identifiers, values must not contain `\n`/`\r`/NUL (blocks injecting extra shell lines). `delete_env_vars` (which also rewrites the `source_path` file) applies the same per-entry validation.
- TS caller inventory (grep): `src/lib/api/settings.ts` (pick_directory, set_app_config_dir_override, save/open_file_dialog, export/import_config_*), `src/lib/api/vscode.ts` (same import/export pair), `src/lib/api/skills.ts` (open_zip_file_dialog, install_skills_from_zip), `src/lib/api/env.ts` (restore_env_backup, using the `backupPath` returned by `delete_env_vars`). All flows obtain paths from the backend dialogs, so no frontend change was needed. Behaviour change: a manually typed app config dir outside the home directory is now rejected on save (use the Browse button).

**Files.** `src-tauri/src/picked_paths.rs` (new), `src-tauri/src/lib.rs`, `src-tauri/src/commands/import_export.rs`, `src-tauri/src/commands/skill.rs`, `src-tauri/src/commands/settings.rs`, `src-tauri/src/commands/config.rs`, `src-tauri/src/app_store.rs` (`resolve_path` made `pub(crate)`), `src-tauri/src/services/env_manager.rs`, `src-tauri/src/services/env_checker.rs`.

**Commits.** `3468052` fix(ipc): validate path arguments of privileged commands (CCS-075); this ticket update in a follow-up docs commit.

**Tests (one per command, reject + accept):**
- export/import: `commands::import_export::tests::{export_rejects_path_not_chosen_by_save_dialog, import_rejects_path_not_chosen_by_open_dialog, export_and_import_accept_dialog_picked_paths}`
- install from zip: `commands::skill::install_from_zip_path_tests::{install_rejects_zip_path_not_chosen_by_dialog, install_accepts_zip_path_chosen_by_dialog}`
- app config dir override: `commands::settings::tests::{app_config_dir_override_rejects_paths_outside_home, app_config_dir_override_accepts_allowed_paths}`
- env restore/delete: `services::env_manager::tests::{restore_rejects_source_path_that_is_not_an_rc_file, restore_rejects_backup_outside_backup_dir, restore_rejects_values_that_inject_extra_lines, restore_appends_export_to_allowed_rc_file, delete_validation_rejects_files_that_are_not_rc_files}`
- registry: `picked_paths::tests::*`

**Verification.**
- `cargo fmt --manifest-path src-tauri/Cargo.toml -- --check`: clean.
- `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings`: clean.
- `cargo test --manifest-path src-tauri/Cargo.toml --lib -- picked_paths import_export:: install_from_zip_path_tests app_config_dir_override env_manager`: 16 passed, 0 failed.
- `cargo test --manifest-path src-tauri/Cargo.toml --no-fail-fast`: 2363 passed, 0 failed.
- `pnpm test:unit`: this change touches no TS. At run time the shared checkout had uncommitted provider-form work from the concurrent frontend lane, and failures there (e.g. `tests/components/GrokBuildProviderForm.test.tsx`) come from that in-progress work, not from this ticket.
