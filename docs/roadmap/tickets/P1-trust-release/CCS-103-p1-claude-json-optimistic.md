# CCS-103: [claude] Use optimistic concurrency for read-modify-write of ~/.claude.json

**Status:** Done · **Kind:** bug · **Labels:** `bug`, `phase:P1`, `p3-low`, `area:apps-claude`

> **Ticket key:** `p1-claude-json-optimistic` · **Phase:** P1 · **Priority:** p3-low · **Effort:** S · **Depends on:** `p1-atomic-write-durable`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Verified finding backend-claude-json-rmw, partially real. Every ~/.claude.json writer does an unlocked whole-file read-modify-write. If Claude Code saves in between, its change is lost. The file is never corrupted.

## Evidence
- src-tauri/src/claude_mcp.rs:345-402 (set_mcp_servers_map), 241/265, 277/285, 153/171, 183/193

## Proposed change
Add a helper: record mtime, length and hash at read; check them again just before atomic_write; if they changed, re-read and re-apply the key-level change, at most 3 times. Route all 5 writers through it.

## Acceptance criteria / verification
- [x] Test with an injected hook that changes `numStartups` between read and write: the final file has both the new mcpServers and numStartups
- [x] `cargo test --manifest-path src-tauri/Cargo.toml claude_mcp`

## Dependencies
p1-atomic-write-durable

## Implementation notes
- **Summary:** Added `update_json_file` in `src-tauri/src/claude_mcp.rs`. It reads `~/.claude.json` and records a fingerprint: length, mtime and SHA-256 of the bytes, or `None` if the file is missing. It then applies a key-level mutation closure (`FnMut(&mut Value, exists) -> Result<bool>`, where `false` means no write is needed). Just before `atomic_write` it fingerprints the file again. If the file changed, it re-reads the file and re-applies the mutation, for at most `MAX_UPDATE_ATTEMPTS = 3` attempts. When the attempts run out it logs a warning and writes the result of the last read, which matches the old last-writer-wins behaviour. All 5 writers now go through it: `set_has_completed_onboarding`, `clear_has_completed_onboarding`, `upsert_mcp_server`, `delete_mcp_server` and `set_mcp_servers_map` (the last one now calls a path-taking `set_mcp_servers_map_at`). Each writer keeps the return values and errors it had before. A `#[cfg(test)]` thread-local `BEFORE_COMMIT_HOOK` runs between the mutation and the re-check, so tests can simulate Claude Code saving the file at that moment.
- **Out of scope:** `src-tauri/src/mcp/claude.rs` still reads the `mcpServers` map with `read_mcp_servers_map` and then calls `set_mcp_servers_map`. That outer read-modify-write only covers the `mcpServers` key, and the final write is now protected for every other key, such as `numStartups`.
- **Files:** `src-tauri/src/claude_mcp.rs`
- **Commits:** 56b0c42 (code + tests); this ticket update in a follow-up docs commit.
- **Tests added:** `set_mcp_servers_map_keeps_concurrent_num_startups_change` (the acceptance test: a hook bumps `numStartups` between read and write, and the final file has both the new `mcpServers` and `numStartups = 6`), `update_json_file_gives_up_after_max_attempts_and_still_applies_change`, `update_json_file_skips_write_when_mutation_reports_no_change`, `update_json_file_creates_missing_file_and_detects_concurrent_creation`.
- **Verification:**
  - `cargo test --manifest-path src-tauri/Cargo.toml --lib claude_mcp` → 16 passed, 0 failed.
  - Regression check: with the fingerprint re-check disabled, the 3 concurrency tests fail. With it restored, they pass.
  - `cargo fmt --check` → clean. `cargo clippy -- -D warnings` → clean.
  - `cargo test --lib` → 2293 passed, 0 failed, 2 ignored.
  - `cargo test --no-fail-fast` (full suite) → 2459 passed, 0 failed.
