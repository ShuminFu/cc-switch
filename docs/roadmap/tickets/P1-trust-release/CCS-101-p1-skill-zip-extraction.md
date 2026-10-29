# CCS-101: [skills] Use enclosed_name and size limits when extracting skill repo archives

**Status:** Done · **Kind:** security · **Labels:** `security`, `phase:P1`, `p3-low`, `area:skills`

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
- [x] Tests: a zip with `repo-main/../../escape.txt` is skipped or errors; a zip over the entry or byte limit gives Err; the symlink-escape test still passes
- [x] `cargo test --manifest-path src-tauri/Cargo.toml skill`

## Dependencies
None.

## Implementation notes
- **Summary:** `download_and_extract` now rejects responses whose `Content-Length` exceeds `MAX_REPO_ARCHIVE_BYTES` (200 MiB) and streams the body chunk-by-chunk into an anonymous temp file with the same cap (no longer buffering the whole archive in memory). Extraction moved into `SkillService::extract_repo_archive(reader, dest, max_entries, max_total_bytes)`: entry paths go through `enclosed_name()` (unsafe entries such as `repo-main/../../escape.txt` or absolute paths are skipped with a warning), the top-level root is removed with `Path::strip_prefix`, the entry count is capped at `MAX_EXTRACT_ENTRIES` (50 000), and extracted bytes (files and symlink targets) are counted through `Read::take` against `MAX_EXTRACT_TOTAL_BYTES` (1 GiB), returning `Err` when exceeded. Existing symlink resolution (`resolve_symlinks_in_dir`, which drops targets outside the repo) is unchanged.
- **Files:** `src-tauri/src/services/skill.rs`
- **Commits:** be88833 (code + tests); this ticket update in a follow-up docs commit.
- **Tests added** (`services::skill::tests`): `extract_repo_archive_skips_entries_escaping_dest` (regression: the old `dest.join(file.name()-root)` wrote `../../escape.txt` outside dest), `extract_repo_archive_rejects_too_many_entries`, `extract_repo_archive_rejects_archive_over_byte_limit`, `extract_repo_archive_resolves_internal_symlinks_and_skips_escaping_ones` (there was no prior symlink-escape test; this one covers it).
- **Verification:**
  - `cargo fmt --manifest-path src-tauri/Cargo.toml` — clean
  - `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings` — no warnings
  - `cargo test --manifest-path src-tauri/Cargo.toml extract_repo_archive` — 4 passed
  - `cargo test --manifest-path src-tauri/Cargo.toml skill` — all passed (lib: 16 passed; integration skill tests: 3 passed)
  - `cargo test --manifest-path src-tauri/Cargo.toml --lib` — 2280 passed, 0 failed, 2 ignored
- **Review:** `enclosed_name()` (zip 2.4) only rejects paths that climb above the *archive* root, so after stripping the top-level folder an entry like `repo-main/../escape.txt` or `repo-main/a/../../escape.txt` became `../escape.txt` and was still written one level outside `dest`. The follow-up commit skips any stripped relative path that contains a non-`Normal` component and adds `extract_repo_archive_skips_entries_escaping_dest_after_root_strip` (files, directories and symlink entries; it failed before the fix). Re-verified: fmt clean, clippy `-D warnings` clean, `cargo test skill` passed, `cargo test --lib` 2281 passed / 0 failed. Known, out of scope: `copy_dir_recursive` in symlink resolution does not guard against a directory symlink pointing at its own ancestor (pre-existing).
