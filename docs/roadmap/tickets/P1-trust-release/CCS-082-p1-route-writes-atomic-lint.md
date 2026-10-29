# CCS-082: [config] Route the remaining config writers through atomic_write and ban std::fs::write

**Status:** Done · **Kind:** refactor · **Labels:** `refactor`, `phase:P1`, `p2-medium`, `area:config`

> **Ticket key:** `p1-route-writes-atomic-lint` · **Phase:** P1 · **Priority:** p2-medium · **Effort:** S · **Depends on:** `p1-atomic-write-perms`, `p0-config-writer-snapshots`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Several writers bypass atomic_write, so the README's atomic-write claim is false. The critique notes that a std::fs::write lint does not catch OpenOptions writers.

## Evidence
- claude_plugin.rs:83/117, services/env_manager.rs:139/220, commands/misc.rs:2773 (settings.rs is handled in p1-settings-atomic-save)

## Proposed change
- Switch these writers to config::atomic_write.
- Add src-tauri/clippy.toml with `disallowed-methods = ["std::fs::write"]`, with test modules exempt.
- Add a CI grep for `OpenOptions::new()` outside config.rs, the token stores and an allowlist.

## Acceptance criteria / verification
- [x] `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings` passes, with zero non-test std::fs::write
- [x] P0 config-writer snapshots are unchanged

## Dependencies
p1-atomic-write-perms, p0-config-writer-snapshots

## Implementation notes

**Summary**
- Routed the remaining production `std::fs::write` callers through `config::atomic_write`:
  `claude_plugin.rs` (write/clear `~/.claude/config.json`), `services/env_manager.rs`
  (env backup JSON, rc-file rewrite on delete, rc-file append on restore),
  `services/config.rs` (config backup), `services/skill.rs` (skill backup `meta.json`),
  `services/webdav_sync/archive.rs` (downloaded skills zip) and the Windows-only
  lifecycle `.bat` in `commands/misc.rs`. The `commands/misc.rs:2773` writer cited in
  the ticket had already moved to `write_private_temp_file` before this ticket.
- Added `src-tauri/clippy.toml` with `disallowed-methods = ["std::fs::write"]`. Test code is
  exempt: unit tests via `#![cfg_attr(test, allow(clippy::disallowed_methods))]` in
  `src/lib.rs` (the plain lib target still lints all production code), integration test
  crates that write fixtures via a file-level `#![allow(clippy::disallowed_methods)]`.
- The only remaining production `std::fs::write` is the `#[cfg(not(unix))]` branch of
  `settings.rs` save, scoped with `#[allow(clippy::disallowed_methods)]` and a comment;
  it is owned by CCS-084 (p1-settings-atomic-save), as the ticket states.
- OpenOptions guard: implemented as the integration test `src-tauri/tests/write_lint.rs`
  (runs in CI's existing `cargo test` step, so no workflow change was needed). It fails on
  `OpenOptions::new(` / `File::options(` in `src/` outside an allowlist (`config.rs`, the
  Copilot and Codex OAuth token stores, `settings.rs` until CCS-084, `panic_hook.rs`
  append-only crash log), checks every allowlist entry still needs it, and self-tests
  the scanner.
- Regression test: `config_writers::claude_plugin_creates_new_config_owner_only` — a fresh
  `~/.claude/config.json` must be 0600 with no temp leftovers. Fails on the previous
  `fs::write` code (file created 0644), passes now.

**Files:** `src-tauri/clippy.toml`, `src-tauri/src/lib.rs`, `src-tauri/src/claude_plugin.rs`,
`src-tauri/src/commands/misc.rs`, `src-tauri/src/services/{config,env_manager,skill}.rs`,
`src-tauri/src/services/webdav_sync/archive.rs`, `src-tauri/src/settings.rs`,
`src-tauri/tests/write_lint.rs` (new), `src-tauri/tests/config_writers.rs`, and the
`#![allow]` header in `src-tauri/tests/{app_config_load,hermes_roundtrip,import_export_sync,mcp_commands,profile_roundtrip,provider_commands,provider_service,proxy_e2e,skill_sync}.rs`.

**Commits:** `26d9ee4` (code + tests), plus this ticket update.

**Verification**
- `cargo fmt --manifest-path src-tauri/Cargo.toml --check` — clean.
- `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings` — passes
  (before the writer changes it reported 8 production `std::fs::write` hits); `cargo clippy
  -- -D warnings` also passes.
- `cargo test --manifest-path src-tauri/Cargo.toml --no-fail-fast` — 2386 passed / 0 failed
  across all targets (lib: 2224 passed, 3 ignored).
- `cargo test --test config_writers` — 17 passed, no `.snap.new` files: P0 config-writer
  snapshots unchanged. The new mode test fails against the pre-change `claude_plugin.rs`
  and passes after.
- `cargo test --test write_lint` — 3 passed.
- Not verifiable locally: clippy on Windows/macOS runners for the cfg-gated `.bat` and
  `settings.rs` non-unix branch (both are written to compile cleanly under the lint).
