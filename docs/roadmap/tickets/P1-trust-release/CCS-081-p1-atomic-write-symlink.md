# CCS-081: [config] Write through symlinked config files instead of replacing the link

**Status:** Open · **Kind:** bug · **Labels:** `bug`, `phase:P1`, `p2-medium`, `area:config`

> **Ticket key:** `p1-atomic-write-symlink` · **Phase:** P1 · **Priority:** p2-medium · **Effort:** S · **Depends on:** `p1-atomic-write-durable`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Verified finding backend-atomic-write-symlink. rename() replaces a symlinked config (stow/chezmoi) with a regular file. The real target silently goes stale. On Windows the link is deleted.

## Evidence
- src-tauri/src/config.rs:297-352 (:346 rename, :326 metadata follows the link); about 58 call sites across 23 files

## Proposed change
If symlink_metadata reports a link, resolve it with canonicalize. For a dangling link, walk read_link with a hop limit. Use the resolved path for the parent, the tmp file, the metadata and the rename.

## Acceptance criteria / verification
- [ ] #[cfg(unix)] tests: absolute, relative and dangling links. The link remains a link, the target has the new content, and no tmp files are left in either dir.
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml atomic_write`

## Dependencies
p1-atomic-write-durable
