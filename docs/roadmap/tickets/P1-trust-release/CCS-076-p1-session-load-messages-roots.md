# CCS-076: [sessions] Apply the provider-root check to load_messages and the sqlite delete paths

**Status:** Done · **Kind:** security · **Labels:** `security`, `phase:P1`, `p3-low`, `area:sessions`

> **Ticket key:** `p1-session-load-messages-roots` · **Phase:** P1 · **Priority:** p3-low · **Effort:** S
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Verified finding backend-load-messages-path, partially real. load_messages and its sqlite variants accept any sourcePath. delete_session checks roots for files but not for sqlite. A compromised webview could read session-formatted content anywhere on disk and check whether a file exists.

## Evidence
- src-tauri/src/session_manager/mod.rs:96-116, 124-132, 151-161; commands/session_manager.rs:14-25

## Proposed change
- Extract validate_source_in_roots(provider_id, path) and use it before dispatch, passing the canonical path to the loader.
- For `sqlite:` sources, require the provider's known DB path.

## Acceptance criteria / verification
- [x] Tests: a JSONL outside the root gives an "outside provider roots" error; inside the root it loads; a sqlite DB outside the known location is rejected for load and for delete
- [x] `cargo test --manifest-path src-tauri/Cargo.toml session_manager`

## Dependencies
None.

## Implementation notes

**Summary.** `session_manager::load_messages` no longer trusts the webview's `sourcePath`:

- File sources: a shared `validate_source_in_roots(provider_id, path, roots)` canonicalizes the source (resolving `..` and symlinks) and requires it to be under one of the provider's session roots (`provider_roots`). It returns the canonical `(root, source)`; both `load_messages` and `delete_session` now dispatch on the canonical path only. Outside paths fail with `Session source path is outside provider roots: ...`.
- `sqlite:` sources (opencode, hermes): `validate_sqlite_source` parses the reference and requires the canonical DB path to equal the provider's known DB (`opencode.db` / hermes `state.db`), for both load and delete. Failure: `SQLite source is outside the known <provider> database: ...`. The session-id mismatch check for delete moved here too. `sqlite:` is only treated as a DB reference for opencode/hermes; other providers go through the root check and are rejected.
- `opencode::load_messages_sqlite` / `delete_session_sqlite` and the hermes equivalents now take an already-validated `&Path` + session id instead of the raw source string, so they can't be called on an unchecked path. The per-provider DB comparison in the delete functions was removed because the central check replaces it. The opencode `delete_session_sqlite_rejects_foreign_db_path` test was replaced by central tests.

**Files.** `src-tauri/src/session_manager/mod.rs`, `src-tauri/src/session_manager/providers/opencode.rs`, `src-tauri/src/session_manager/providers/hermes.rs`.

**Commits.** `9aa64e4` (code + tests); this ticket update is a follow-up docs commit.

**Tests added (session_manager::tests).** `load_messages_rejects_jsonl_outside_provider_roots`, `load_messages_reads_jsonl_inside_provider_root`, `load_messages_rejects_parent_traversal_out_of_root`, `load_messages_rejects_symlink_escaping_root` (unix), `sqlite_load_reads_known_database`, `sqlite_load_rejects_database_outside_known_location` (hermes + opencode), `sqlite_load_rejects_missing_database`, `sqlite_delete_rejects_database_outside_known_location` (also checks the foreign DB is left as it was), `sqlite_delete_removes_session_from_known_database` (including the id mismatch case), `sqlite_prefix_is_not_a_database_source_for_file_providers`.

**Verification.**
- `cargo fmt --manifest-path src-tauri/Cargo.toml`: clean
- `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings`: no warnings
- `cargo test --manifest-path src-tauri/Cargo.toml --lib session_manager`: 80 passed, 0 failed
- `cargo test --manifest-path src-tauri/Cargo.toml session_manager` (all targets): 80 passed, 0 failed
- `cargo test --manifest-path src-tauri/Cargo.toml --lib`: 2215 passed, 0 failed, 3 ignored
