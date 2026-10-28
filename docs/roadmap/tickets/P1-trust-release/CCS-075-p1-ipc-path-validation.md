# CCS-075: [ipc] Validate path arguments of privileged commands against allowed roots or dialog-issued tokens

**Status:** Open · **Kind:** security · **Labels:** `security`, `phase:P1`, `p1-high`, `area:ipc`

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
