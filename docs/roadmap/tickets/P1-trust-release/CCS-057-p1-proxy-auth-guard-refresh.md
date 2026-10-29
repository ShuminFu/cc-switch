# CCS-057: [proxy] Stop holding the Copilot/Codex auth read guard across network token refresh

**Status:** Done · **Kind:** bug · **Labels:** `bug`, `phase:P1`, `p3-low`, `area:proxy`

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
- [x] Test: during a token fetch against a 2 s-delayed mock, `write()` with a 200 ms timeout succeeds
- [x] `cargo test --manifest-path src-tauri/Cargo.toml proxy::` and clippy

## Dependencies
None.

## Implementation notes

**Summary.** `CopilotAuthManager` and `CodexOAuthManager` now `#[derive(Clone)]` (every mutable field is an `Arc<RwLock<..>>`, plus the `PathBuf` storage path), so a clone shares the same state. `CopilotAuthState::manager()` / `CodexOAuthState::manager()` clone the manager under the outer read guard and drop it before returning. All five forwarder call sites (dynamic endpoint routing, Copilot token, Codex OAuth token, live model resolution, vendor lookup) use `manager()`, so no outer guard is held across a network refresh. The Copilot/auth Tauri commands that took `write()` (poll, remove account, set default, logout, for both providers) now take `read()`, since the managers only need interior locking. No code path takes the outer write lock any more.

For the test, `CopilotAuthManager` has a `#[cfg(test)]` `api_base_override` field that sends the Copilot token exchange to a local mock. Release builds do not include it.

**Files**
- `src-tauri/src/proxy/providers/copilot_auth.rs`: `Clone`, test-only API base override, regression test `test_token_refresh_does_not_hold_outer_guard`
- `src-tauri/src/proxy/providers/codex_oauth_auth.rs`: `Clone`
- `src-tauri/src/commands/copilot.rs`, `src-tauri/src/commands/codex_oauth.rs`: `manager()` helpers; `write()` changed to `read()`
- `src-tauri/src/commands/auth.rs`: `write()` changed to `read()`
- `src-tauri/src/proxy/forwarder.rs`: uses `manager()` at every call site

**Commits:** 9b29133 (code), plus the follow-up commit that records these notes.

**Verification**
- The regression test runs against a mock that waits 2 s before answering the token exchange. Under the old pattern (outer read guard held during the refresh), `write()` with a 200 ms timeout times out. This confirms the mock really delays the refresh. Under the new `manager()` pattern, `write()` with a 200 ms timeout succeeds while the refresh is still running, and the refresh then returns the fresh token.
- `cargo fmt`: clean. `cargo clippy -- -D warnings` and `cargo clippy --tests -- -D warnings`: clean.
- `cargo test proxy::`: 1185 passed, 0 failed.
- `cargo test --lib`: 2116 passed, 0 failed, 3 ignored.
- `cargo test --no-fail-fast` (full suite): 2272 passed, 0 failed.
