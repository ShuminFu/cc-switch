# CCS-103: [claude] Use optimistic concurrency for read-modify-write of ~/.claude.json

**Status:** Open · **Kind:** bug · **Labels:** `bug`, `phase:P1`, `p3-low`, `area:apps-claude`

> **Ticket key:** `p1-claude-json-optimistic` · **Phase:** P1 · **Priority:** p3-low · **Effort:** S · **Depends on:** `p1-atomic-write-durable`
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Verified finding backend-claude-json-rmw, partially real. Every ~/.claude.json writer does an unlocked whole-file read-modify-write. If Claude Code saves in between, its change is lost. The file is never corrupted.

## Evidence
- src-tauri/src/claude_mcp.rs:345-402 (set_mcp_servers_map), 241/265, 277/285, 153/171, 183/193

## Proposed change
Add a helper: record mtime, length and hash at read; check them again just before atomic_write; if they changed, re-read and re-apply the key-level change, at most 3 times. Route all 5 writers through it.

## Acceptance criteria / verification
- [ ] Test with an injected hook that changes `numStartups` between read and write: the final file has both the new mcpServers and numStartups
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml claude_mcp`

## Dependencies
p1-atomic-write-durable
