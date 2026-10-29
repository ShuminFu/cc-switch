# CCS-080: [config] Create new files written by atomic_write with 0600 permissions

**Status:** Done · **Kind:** security · **Labels:** `security`, `phase:P1`, `p1-high`, `area:config`

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
- [x] #[cfg(unix)] test: a new path gives mode & 0o777 == 0o600; a pre-existing 0640 target stays 0640
- [x] `cargo test --manifest-path src-tauri/Cargo.toml atomic_write`

## Dependencies
p1-atomic-write-durable (same function)

## Implementation notes

**Summary:** `atomic_write` (src-tauri/src/config.rs) now builds its temp file with
`OpenOptions::new().write(true).create_new(true)` plus, on unix, `.mode(0o600)`
(via `OpenOptionsExt`). A brand-new target therefore lands as 0600 instead of the
umask default (0644). When the target already exists, `write_and_sync` still copies
its mode onto the temp file, so e.g. a 0640 file stays 0640. The optional "create
~/.codex as 0700" item was not done: directories are still created by `create_dir_all`
with umask permissions. That item was optional and is left out to keep this change small.

**Files:** `src-tauri/src/config.rs` (fix + new unix test
`atomic_write_creates_new_files_owner_only`, which checks a new path gives 0600 and a
pre-existing 0640 target stays 0640).

**Commits:** d229f35 (code + test); this ticket update follows in a docs commit.

**Verification:**
- Regression check: with the `.mode(0o600)` line disabled, the new test fails
  (`left: 420` = 0o644, `right: 384` = 0o600, umask 0022); with the fix it passes.
- `cargo test --manifest-path src-tauri/Cargo.toml atomic_write`: 4 passed, 0 failed.
- `cargo fmt --manifest-path src-tauri/Cargo.toml`: clean.
- `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings`: no warnings.
- `cargo test --manifest-path src-tauri/Cargo.toml --no-fail-fast`: 2378 passed, 0 failed.
