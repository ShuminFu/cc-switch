# CCS-096: [usage] Advance the session-log offset only past complete newline-terminated lines

**Status:** Done · **Kind:** bug · **Labels:** `bug`, `phase:P1`, `p2-medium`, `area:usage`

> **Ticket key:** `p1-session-sync-partial-line` · **Phase:** P1 · **Priority:** p2-medium · **Effort:** S
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Verified finding backend-session-usage-partial-line. A trailing line that is still being written fails to parse but is still counted in line_offset. It is skipped forever once complete.

## Evidence
- src-tauri/src/services/session_usage.rs:181-355 (~207 line_offset += 1, ~223 Err continue, ~352 update_sync_state)

## Proposed change
- Read with read_until(b'\n') and count only lines that end in '\n'.
- Consider a byte offset plus seek.
- Apply the same change to the other session_usage_* parsers that share get_sync_state.

## Acceptance criteria / verification
- [x] Test: one complete line plus a truncated second line gives 1 import; complete it, bump the mtime, and the second is imported
- [x] `cargo test --manifest-path src-tauri/Cargo.toml session_usage`

## Dependencies
None.

## Implementation notes
- **Summary:** Added `CompleteLines` (in `session_usage.rs`), a `BufRead` iterator built on `read_until(b'\n')` that yields only newline-terminated lines (with `\n`/`\r\n` stripped) and stops at an unterminated tail. Non-UTF-8 complete lines are yielded as `Err` so they are counted and skipped. The Claude importer (`sync_single_file`), the Codex importer (`sync_single_codex_file`), and the Codex history-replay boundary scan now use it. A partially written last line is no longer counted in `last_line_offset`. Once the writer finishes it, the mtime changes and the next sync imports it. Line-count offsets are kept (no schema change). The Gemini importer parses whole-file JSON and the OpenCode importer reads SQLite; neither uses line offsets, so both are unchanged.
- **Files:** `src-tauri/src/services/session_usage.rs`, `src-tauri/src/services/session_usage_codex.rs`
- **Commits:** `25dc065` (code + tests), plus this ticket update.
- **Tests:** `test_sync_defers_partial_trailing_line_until_complete` (Claude), `test_codex_defers_partial_trailing_line_until_complete` (Codex), `test_complete_lines_skips_unterminated_tail`. With the old behavior simulated (partial tail yielded and counted), both regression tests fail.
- **Verification:**
  - `cargo fmt`: clean.
  - `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings`: passed.
  - `cargo test --manifest-path src-tauri/Cargo.toml --lib session_usage`: 50 passed, 0 failed.
  - `cargo test --manifest-path src-tauri/Cargo.toml --lib`: 2269 passed, 0 failed, 2 ignored.
