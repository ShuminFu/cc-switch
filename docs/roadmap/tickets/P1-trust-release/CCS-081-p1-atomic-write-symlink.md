# CCS-081: [config] Write through symlinked config files instead of replacing the link

**Status:** Done · **Kind:** bug · **Labels:** `bug`, `phase:P1`, `p2-medium`, `area:config`

> **Ticket key:** `p1-atomic-write-symlink` · **Phase:** P1 · **Priority:** p2-medium · **Effort:** S · **Depends on:** `p1-atomic-write-durable`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Verified finding backend-atomic-write-symlink. rename() replaces a symlinked config (stow/chezmoi) with a regular file. The real target silently goes stale. On Windows the link is deleted.

## Evidence
- src-tauri/src/config.rs:297-352 (:346 rename, :326 metadata follows the link); about 58 call sites across 23 files

## Proposed change
If symlink_metadata reports a link, resolve it with canonicalize. For a dangling link, walk read_link with a hop limit. Use the resolved path for the parent, the tmp file, the metadata and the rename.

## Acceptance criteria / verification
- [x] #[cfg(unix)] tests: absolute, relative and dangling links. The link remains a link, the target has the new content, and no tmp files are left in either dir.
- [x] `cargo test --manifest-path src-tauri/Cargo.toml atomic_write`

## Dependencies
p1-atomic-write-durable

## Implementation notes

**Summary.** `atomic_write` (src-tauri/src/config.rs) now calls a new helper, `resolve_write_target`, before doing anything else. If the path is not a symlink, or does not exist, it is used as given. A live link is resolved with `fs::canonicalize`. A dangling link is followed one `read_link` hop at a time, with relative targets resolved against the link's own directory, until the walk reaches a path that is not a link. The walk is capped at `MAX_SYMLINK_HOPS = 40`, and a loop returns `AppError::Config`. The resolved path is then used for `create_dir_all`, the temp file (so it lands next to the real target, on the same filesystem), the permission copy and the rename. The link itself is never touched. Because all ~58 call sites go through `atomic_write` / `write_json_file` / `write_text_file`, none of them needed changes.

**Files.** `src-tauri/src/config.rs`.

**Commits.** `527e475` fix(config): write through symlinked config files in atomic_write (CCS-081). The ticket record is in the follow-up docs commit.

**Tests** (`#[cfg(unix)]`, in `config::tests`):
- `atomic_write_follows_absolute_symlink`: the link stays a link, the target gets the new content, the target's 0640 mode is kept, and neither dir has tmp files left.
- `atomic_write_follows_relative_symlink_chain`: a two-hop relative chain (`home/config.toml -> .hop -> ../dotfiles/config.toml`). Both links survive and the target is updated.
- `atomic_write_creates_target_of_dangling_symlink`: a relative dangling link. The target is created, the link survives, and no tmp files are left.
- `atomic_write_rejects_symlink_loop`: `a <-> b` returns an error, the links are untouched, and no tmp files are left.

Before the fix the first three fail on the "link was replaced by a file" assertion, and the loop test fails because the rename used to succeed.

**Verification.**
- `cargo fmt --manifest-path src-tauri/Cargo.toml`: clean.
- `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings`: clean.
- `cargo test --manifest-path src-tauri/Cargo.toml --lib atomic_write`: 8 passed, 0 failed.
- `cargo test --manifest-path src-tauri/Cargo.toml --no-fail-fast`: lib 2224 passed / 0 failed / 3 ignored, and all integration test binaries passed except one intermittent failure, described below.

**Unrelated flake.** `tests/migrations.rs::every_released_schema_migrates_to_current` fails intermittently, about 1 run in 4 when the file is run on its own. The failure is "v2 fixture: row not preserved in proxy_request_logs". It is in the DB migration / log-retention path and does not involve `atomic_write`. It also passes in isolation, so it was not changed here.

**Not covered locally.** The Windows behaviour (the link used to be deleted) gets the same code path, because `canonicalize` / `read_link` are cross-platform. However, the tests are unix-only, so this has not been run on a Windows runner.
