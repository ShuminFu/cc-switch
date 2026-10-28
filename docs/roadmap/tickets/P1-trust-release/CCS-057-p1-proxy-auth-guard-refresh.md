# CCS-057: [proxy] Stop holding the Copilot/Codex auth read guard across network token refresh

**Status:** Open · **Kind:** bug · **Labels:** `bug`, `phase:P1`, `p3-low`, `area:proxy`

> **Ticket key:** `p1-proxy-auth-guard-refresh` · **Phase:** P1 · **Priority:** p3-low · **Effort:** S
> Plan: [next-phase-plan.md](../../next-phase-plan.md)

## Problem
Verified finding proxy-auth-guard-refresh. The forwarder holds the outer CopilotAuthState/CodexOAuthState read guard while get_valid_token* does a network refresh. UI commands that take the write lock block, and because tokio's RwLock is fair, later proxy reads also block.

## Evidence
- src-tauri/src/proxy/forwarder.rs:1572-1591, 1623-1639 (also 1298, 2470, 2504); copilot_auth.rs:714-773; writers commands/copilot.rs:43,72,105,118,146 and commands/auth.rs:121,137,243,250,270

## Proposed change
- Derive Clone on the managers (all fields are Arc) or hold Arc<Manager>.
- Clone the manager under the guard, drop the guard, then await.
- Change the command write() locks to read() where only interior locking is needed.

## Acceptance criteria / verification
- [ ] Test: during a token fetch against a 2 s-delayed mock, `write()` with a 200 ms timeout succeeds
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml proxy::` and clippy

## Dependencies
None.
