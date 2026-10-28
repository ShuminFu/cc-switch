# CCS-101: [skills] Use enclosed_name and size limits when extracting skill repo archives

**Status:** Open · **Kind:** security · **Labels:** `security`, `phase:P1`, `p3-low`, `area:skills`

> **Ticket key:** `p1-skill-zip-extraction` · **Phase:** P1 · **Priority:** p3-low · **Effort:** S
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Verified finding backend-skill-zip-slip. Remote extraction joins the raw file.name() onto dest, buffers the whole archive in memory, and has no entry or byte limits. A MITM, a malicious proxy or a future non-GitHub source would make this a zip-slip.

## Evidence
- src-tauri/src/services/skill.rs:2238-2307 (2256 bytes(), 2276 file.name(), 2289 join); safe patterns skill.rs:2697 and webdav_sync/archive.rs:18,92,115

## Proposed change
- Extract a helper that takes a reader and dest.
- Use enclosed_name and strip the root via PathBuf::strip_prefix.
- Cap entries at MAX_EXTRACT_ENTRIES, enforce a byte limit through Read::take, and check Content-Length.

## Acceptance criteria / verification
- [ ] Tests: a zip with `repo-main/../../escape.txt` is skipped or errors; a zip over the entry or byte limit gives Err; the symlink-escape test still passes
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml skill`

## Dependencies
None.
