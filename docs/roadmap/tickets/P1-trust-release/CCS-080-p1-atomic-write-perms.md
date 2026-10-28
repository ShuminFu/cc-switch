# CCS-080: [config] Create new files written by atomic_write with 0600 permissions

**Status:** Open · **Kind:** security · **Labels:** `security`, `phase:P1`, `p1-high`, `area:config`

> **Ticket key:** `p1-atomic-write-perms` · **Phase:** P1 · **Priority:** p1-high · **Effort:** S · **Depends on:** `p1-atomic-write-durable`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Verified finding backend-atomic-write-perms. The critique corrects the plan: atomic_write already preserves the mode of an existing file. The gap is new files. The temp file gets umask permissions (0644), so a first-time ~/.codex/auth.json holding OPENAI_API_KEY, or a Claude settings.json with a token, is readable by other local users.

## Evidence
- src-tauri/src/config.rs:318 File::create, 323-330 copies permissions only when the target exists; codex_config.rs:244,250; secure examples gemini_config.rs:185, settings.rs:687

## Proposed change
- On unix, open the temp file with `OpenOptions::new().write(true).create_new(true).mode(0o600)`.
- Keep the existing mode when the target exists.
- Optionally create ~/.codex as 0700 when the app creates it.

## Acceptance criteria / verification
- [ ] #[cfg(unix)] test: a new path gives mode & 0o777 == 0o600; a pre-existing 0640 target stays 0640
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml atomic_write`

## Dependencies
p1-atomic-write-durable (same function)
