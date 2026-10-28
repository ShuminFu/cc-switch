# CCS-076: [sessions] Apply the provider-root check to load_messages and the sqlite delete paths

**Status:** Open · **Kind:** security · **Labels:** `security`, `phase:P1`, `p3-low`, `area:sessions`

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
- [ ] Tests: a JSONL outside the root gives an "outside provider roots" error; inside the root it loads; a sqlite DB outside the known location is rejected for load and for delete
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml session_manager`

## Dependencies
None.
